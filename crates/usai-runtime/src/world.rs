//! The execution world driver.
//!
//! One `WorldDriver` is the single owner of one guest instance. It admits
//! completions through one identity-first gate, decides when the world's work
//! is terminal, detects work the world tried to leave behind, and retires the
//! world on every path — including being dropped mid-flight.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::definition::{ApplicationDefinition, LifetimeFamily, WorkloadSpec};
use crate::engine::{
    Compiled, Engine, EngineError, GuestError, GuestState, HostBindings, Outcome, WorldInstance,
};
use crate::host_ops::{ChildRecord, Completion, OpContext, OpExtensions, spawn_operation};
use crate::ownership::{Gauges, Ledger, OpId, WorldId, dec, inc};
use crate::resource::BoundResources;

/// A lifecycle rule the world violated. The message teaches the model
/// (`GOAL.md` §45); the code is stable for tooling.
#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LifecycleViolation {
    pub code: &'static str,
    pub message: String,
    /// Whether the cut-off work included an external side effect whose
    /// terminal state is now unknown (a resource operation, a queue
    /// publish, an owned invoke, an open transaction) — as opposed to a
    /// timer or a pure computation. A finite HTTP world must not answer
    /// success over a cancelled write.
    pub side_effects_lost: bool,
}

impl LifecycleViolation {
    pub fn detached_work(workload: &WorkloadSpec, kinds: &[String]) -> Self {
        let kind = workload.trigger.kind_name();
        let mut summary = std::collections::BTreeMap::<&str, usize>::new();
        for k in kinds {
            *summary.entry(k.as_str()).or_default() += 1;
        }
        let live = summary
            .iter()
            .map(|(k, n)| format!("{n} {k}"))
            .collect::<Vec<_>>()
            .join(", ");
        // A pending `task.dispatch` is a hand-off whose acknowledgement was
        // not awaited: the transfer already happened and the task runs. Every
        // other pending kind (timers, owned invokes, resource calls) was cut
        // off with the world. Say which.
        let only_dispatch = summary.keys().all(|k| *k == "task.dispatch");
        let open_transaction = summary.contains_key("postgres.transaction");
        let outcome = if open_transaction {
            "A database transaction was still open when the handler returned. The runtime rolled              it back on the world's behalf — nothing it wrote is durable. Await `db.transaction(...)`              so it commits before the response, or roll back explicitly by throwing inside it."
                .to_owned()
        } else if only_dispatch {
            "The hand-off itself already happened — the dispatched task runs in its own world — \
             but this world returned before the runtime acknowledged it. Add `await` in front of \
             `ctx.tasks.dispatch(...)` so the request only answers once the transfer is recorded."
                .to_owned()
        } else {
            format!(
                "The {kind} lifetime ended when its result was produced. Work that is still \
                 pending cannot remain owned by this world, so it was cancelled.\n\n\
                 Use:\n  \
                 task()    for independent finite work (await ctx.tasks.dispatch(task, input): the hand-off is awaited, the work runs on its own)\n  \
                 cron()    for scheduled work\n  \
                 service() for intentional long-running work\n\
                 or await the work before returning."
            )
        };
        let side_effects_lost = summary.keys().any(|k| {
            matches!(
                *k,
                "resource" | "queue.publish" | "task.invoke" | "postgres.transaction"
            )
        });
        Self {
            code: "detached_work",
            message: format!(
                "{kind} work `{}` ended with live asynchronous work ({live}).\n\n{outcome}",
                workload.name
            ),
            side_effects_lost,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Termination {
    /// The handler settled on its own.
    Completed,
    /// The world was cancelled by its owner (client disconnect, shutdown).
    Cancelled { reason: String },
    /// The per-invocation deadline fired.
    DeadlineExceeded,
    /// The engine or bridge failed; the world cannot be trusted.
    Faulted { detail: String },
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkResult {
    pub world: WorldId,
    pub workload: String,
    pub termination: Termination,
    /// `Ok(value)` from the handler, `Err(guest error)` when it threw. `None`
    /// when the world never reached a terminal outcome (cancelled, faulted).
    pub outcome: Option<Result<serde_json::Value, GuestError>>,
    pub violations: Vec<LifecycleViolation>,
    pub duration: Duration,
    pub completions_delivered: u32,
    pub completions_dropped: u32,
    pub logs: Vec<LogLine>,
    /// Child work this world started (owned invocations, transferred dispatches).
    pub children: Vec<ChildRecord>,
    /// Per-phase time accounting (`USAI_PROFILE=1`): driver phases plus
    /// the engine's own, in milliseconds.
    pub profile: Vec<(String, f64)>,
    /// Thread CPU time spent inside guest entries (the world's own CPU;
    /// host operations run elsewhere and are not included).
    pub cpu: Duration,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct LogLine {
    pub level: String,
    pub message: String,
}

/// State the guest natives reach through `HostBindings`. Everything here is
/// safe to call from inside the engine while the driver is inside `with`.
pub struct WorldShared {
    pub id: WorldId,
    ledger: Arc<Ledger>,
    cancel: CancellationToken,
    resources: Arc<BoundResources>,
    extensions: Arc<OpExtensions>,
    completions: mpsc::Sender<Completion>,
    revision: Option<Arc<crate::runtime::Revision>>,
    children: Arc<Mutex<Vec<ChildRecord>>>,
    attachment: Option<Arc<dyn std::any::Any + Send + Sync>>,
    logs: Mutex<Vec<LogLine>>,
    accepting_ops: AtomicBool,
    max_logs: usize,
    /// The workload id, so an application log line says which workload
    /// wrote it without the application embedding the name itself.
    workload: Arc<str>,
}

impl HostBindings for WorldShared {
    fn start(&self, kind: &str, payload: &str) -> i64 {
        if !self.accepting_ops.load(Ordering::SeqCst) {
            return 0;
        }
        let op = self.ledger.start(self.id, kind);
        let ctx = OpContext {
            world: self.id,
            op,
            cancel: self.cancel.child_token(),
            resources: Arc::clone(&self.resources),
            extensions: Arc::clone(&self.extensions),
            revision: self.revision.clone(),
            children: Arc::clone(&self.children),
            attachment: self.attachment.clone(),
        };
        match spawn_operation(
            &self.ledger,
            ctx,
            kind,
            payload.to_owned(),
            self.completions.clone(),
        ) {
            Ok(op) => op.0 as i64,
            Err(refusal) => {
                // Nothing was spawned, so nothing will release the record.
                self.ledger.release(op);
                tracing::debug!(world = %self.id, kind, refusal = %refusal.payload, "operation refused");
                0
            }
        }
    }

    fn cancel_op(&self, op: u64) {
        // Guest-side cancel (clearTimeout). The owner still reaches terminal
        // state; its completion is simply no longer deliverable.
        let _ = self.ledger.cancel_op(OpId(op), self.id);
    }

    fn log(&self, level: &str, message: &str) {
        let mut logs = self.logs.lock().expect("logs poisoned");
        if logs.len() < self.max_logs {
            logs.push(LogLine {
                level: level.to_owned(),
                message: message.to_owned(),
            });
        }
        // The application's own lines keep their level and carry a target
        // of their own (`app`), so the default filter shows them at INFO in
        // `dev` and `run` while the runtime's internals stay at their level.
        let workload = &*self.workload;
        match level {
            "error" => tracing::error!(target: "app", workload, world = %self.id, "{message}"),
            "warn" => tracing::warn!(target: "app", workload, world = %self.id, "{message}"),
            "debug" => tracing::debug!(target: "app", workload, world = %self.id, "{message}"),
            _ => tracing::info!(target: "app", workload, world = %self.id, "{message}"),
        }
    }
}

pub struct WorldSpec {
    pub definition: Arc<ApplicationDefinition>,
    pub compiled: Arc<dyn Compiled>,
    pub workload_index: usize,
    pub resources: Arc<BoundResources>,
    pub extensions: Arc<OpExtensions>,
    pub deadline: Option<Duration>,
    /// Hard bound on one uninterrupted synchronous guest run.
    pub cpu_slice: Duration,
    pub cancel: CancellationToken,
    /// Graceful stop request for persistent workloads (`None` for finite work).
    pub stop: Option<CancellationToken>,
    pub revision: Option<Arc<crate::runtime::Revision>>,
    pub attachment: Option<Arc<dyn std::any::Any + Send + Sync>>,
}

pub struct WorldDriver {
    id: WorldId,
    definition: Arc<ApplicationDefinition>,
    workload_index: usize,
    instance: Box<dyn WorldInstance>,
    shared: Arc<WorldShared>,
    completions: mpsc::Receiver<Completion>,
    ledger: Arc<Ledger>,
    gauges: Arc<Gauges>,
    deadline: Option<Duration>,
    cpu_slice: Duration,
    cancel: CancellationToken,
    stop: Option<CancellationToken>,
    delivered: u32,
    dropped: u32,
    finished: bool,
    instantiate: Duration,
    watch: Arc<WatchSlot>,
    /// When the deadline elapses, as an instant, so synchronous guest runs
    /// can be bounded by it too (see `watchdog`).
    deadline_at: Option<Instant>,
    /// The guest state the drive loop read when the handler settled, so the
    /// end of `run` does not read it again.
    last_state: Option<GuestState>,
}

impl WorldDriver {
    pub async fn create(
        engine: &dyn Engine,
        ledger: Arc<Ledger>,
        spec: WorldSpec,
    ) -> Result<Self, EngineError> {
        let id = ledger.next_world_id();
        let (tx, rx) = mpsc::channel(64);
        let workload: Arc<str> = Arc::from(
            spec.definition
                .workloads()
                .get(spec.workload_index)
                .map(|w| w.id.as_str())
                .unwrap_or(""),
        );
        let shared = Arc::new(WorldShared {
            id,
            ledger: Arc::clone(&ledger),
            cancel: spec.cancel.clone(),
            resources: spec.resources,
            extensions: spec.extensions,
            completions: tx,
            revision: spec.revision,
            children: Arc::new(Mutex::new(Vec::new())),
            attachment: spec.attachment,
            logs: Mutex::new(Vec::new()),
            accepting_ops: AtomicBool::new(true),
            max_logs: 1_000,
            workload,
        });
        let bindings: Arc<dyn HostBindings> = Arc::clone(&shared) as Arc<dyn HostBindings>;
        let t_inst = std::time::Instant::now();
        let instance = engine.instantiate(&spec.compiled, bindings).await?;
        let watch = WatchSlot::register(instance.interrupter());
        tracing::debug!(
            engine_instantiate_ms = t_inst.elapsed().as_secs_f64() * 1000.0,
            "driver create"
        );
        // Every fallible step is done: the live-world gauge rises only once
        // the driver that lowers it exists.
        let gauges = Arc::clone(&ledger.gauges);
        inc(&gauges.live_worlds);
        inc(&gauges.worlds_created);
        tracing::debug!(world = %id, workload = spec.definition.workload_by_index(spec.workload_index).map(|w| w.id.as_str()).unwrap_or("?"), "world created");
        Ok(Self {
            id,
            definition: spec.definition,
            workload_index: spec.workload_index,
            instance,
            shared,
            completions: rx,
            ledger,
            gauges,
            deadline: spec.deadline,
            cpu_slice: spec.cpu_slice,
            cancel: spec.cancel,
            stop: spec.stop,
            delivered: 0,
            dropped: 0,
            finished: false,
            instantiate: t_inst.elapsed(),
            watch,
            deadline_at: spec.deadline.map(|d| Instant::now() + d),
            last_state: None,
        })
    }

    pub fn id(&self) -> WorldId {
        self.id
    }

    fn workload(&self) -> &WorkloadSpec {
        self.definition
            .workload_by_index(self.workload_index)
            .expect("workload index was validated at admission")
    }

    /// Arms the watchdog for one guest entry: the CPU slice, or what is
    /// left of the deadline if that is shorter, so a deadline also bounds
    /// synchronous work that never reaches an `await`.
    fn watchdog(&self) -> Watchdog {
        let remaining = self
            .deadline_at
            .map(|at| at.saturating_duration_since(Instant::now()))
            .unwrap_or(Duration::MAX);
        Watchdog::arm(
            &self.watch,
            self.cpu_slice.min(remaining.max(Duration::from_millis(1))),
        )
    }

    /// Whether a guest fault happened because the deadline elapsed while the
    /// guest was running synchronously (the watchdog interrupted it).
    fn deadline_elapsed(&self) -> bool {
        self.deadline_at.is_some_and(|at| Instant::now() >= at)
    }

    /// The one routing gate. Identity first, then ledger deliverability,
    /// then the guest.
    async fn route(&mut self, completion: Completion) -> Result<(), EngineError> {
        let Completion {
            world,
            op,
            outcome,
            guard,
        } = completion;
        if world != self.id {
            inc(&self.gauges.completions_rejected_stale);
            self.dropped += 1;
            drop(guard);
            return Ok(());
        }
        if !self.ledger.is_deliverable(self.id, op) {
            inc(&self.gauges.completions_dropped_late);
            self.dropped += 1;
            drop(guard);
            return Ok(());
        }
        let watchdog = self.watchdog();
        let accepted = watchdog.finish(
            self.instance
                .deliver(op.0, outcome.ok, &outcome.payload)
                .await,
        )?;
        // Released after delivery, so a concurrent late duplicate could not
        // have been judged deliverable while the guest was still waiting.
        guard.release();
        if accepted {
            self.delivered += 1;
            inc(&self.gauges.completions_delivered);
        } else {
            self.dropped += 1;
            inc(&self.gauges.completions_dropped_late);
        }
        Ok(())
    }

    /// Drives the workload to its terminal state. Consumes the driver, so
    /// retirement happens exactly once, here or in `Drop`.
    pub async fn run(mut self, input: &serde_json::Value) -> WorkResult {
        let started = Instant::now();
        let workload_id = self.workload().id.clone();
        let input_json = input.to_string();
        let index = self.workload_index;

        let watchdog = self.watchdog();
        let termination = match watchdog.finish(self.instance.invoke(index, &input_json).await) {
            Ok(()) => self.drive().await,
            Err(_) if self.deadline_elapsed() => self.deadline_interrupted().await,
            Err(e) => Termination::Faulted {
                detail: e.to_string(),
            },
        };

        // One read of the guest at the end: the driver's loop already
        // fetched the settled state, so this is only a call when the
        // world ended some other way (deadline, cancel, fault).
        let state = match self.last_state.take() {
            Some(state) => Some(state),
            None => self.instance.state().await.ok(),
        };
        let mut guest_profile = Vec::new();
        let (outcome, pending) = match state {
            Some(GuestState { outcome, pending }) => (
                match outcome {
                    Some(Outcome::Ok { value, profile, .. }) => {
                        guest_profile = profile;
                        Some(Ok(value))
                    }
                    Some(Outcome::Err { error, profile, .. }) => {
                        guest_profile = profile;
                        Some(Err(self.definition.map_error(error)))
                    }
                    None => None,
                },
                Some(pending),
            ),
            None => (None, None),
        };

        let mut violations = Vec::new();
        if matches!(termination, Termination::Completed)
            && self.workload().lifetime() == LifetimeFamily::Finite
            && let Some(pending) = pending
            && pending.count > 0
        {
            inc(&self.gauges.detached_work_detected);
            violations.push(LifecycleViolation::detached_work(
                self.workload(),
                &pending.kinds,
            ));
            let _ = self
                .instance
                .cancel("finite work ended with live asynchronous work")
                .await;
        }

        let logs = std::mem::take(&mut *self.shared.logs.lock().expect("logs poisoned"));
        let children =
            std::mem::take(&mut *self.shared.children.lock().expect("children poisoned"));
        let profile = if crate::engine::profiling() {
            let mut p: Vec<(String, f64)> = self
                .instance
                .phases()
                .into_iter()
                .map(|(k, d)| (format!("engine.{k}"), d.as_secs_f64() * 1000.0))
                .collect();
            p.push((
                "driver.instantiate".into(),
                self.instantiate.as_secs_f64() * 1000.0,
            ));
            p.push((
                "driver.run".into(),
                started.elapsed().as_secs_f64() * 1000.0,
            ));
            p.extend(
                guest_profile
                    .into_iter()
                    .map(|(k, ms)| (format!("guest.{k}"), ms)),
            );
            p
        } else {
            Vec::new()
        };
        let cpu = Duration::from_nanos(self.watch.cpu_ns.load(Ordering::Relaxed));
        self.gauges
            .guest_cpu_ns
            .fetch_add(cpu.as_nanos() as u64, Ordering::Relaxed);
        let mut result = WorkResult {
            world: self.id,
            workload: workload_id,
            termination,
            outcome,
            violations,
            duration: started.elapsed(),
            completions_delivered: self.delivered,
            completions_dropped: self.dropped,
            logs,
            children,
            profile,
            cpu,
        };
        let t_retire = Instant::now();
        self.retire("finished");
        if crate::engine::profiling() {
            result.profile.push((
                "driver.retire".into(),
                t_retire.elapsed().as_secs_f64() * 1000.0,
            ));
        }
        result
    }

    async fn drive(&mut self) -> Termination {
        let deadline = self.deadline.map(|d| tokio::time::sleep(d));
        tokio::pin!(deadline);
        let mut stop = self.stop.clone();
        loop {
            match self.instance.state().await {
                Ok(state) if state.outcome.is_some() => {
                    self.last_state = Some(state);
                    return Termination::Completed;
                }
                Ok(_) => {}
                Err(e) => {
                    return Termination::Faulted {
                        detail: e.to_string(),
                    };
                }
            }
            tokio::select! {
                biased;
                _ = self.cancel.cancelled() => {
                    let reason = "cancelled by owner".to_owned();
                    return self.cancel_world(&reason).await.unwrap_or_else(|e| Termination::Faulted { detail: e.to_string() });
                }
                _ = async { match &stop { Some(s) => s.cancelled().await, None => std::future::pending().await } } => {
                    // Ask once; afterwards only the hard cancel path ends the world.
                    stop = None;
                    let watchdog = self.watchdog();
                    if let Err(e) = watchdog.finish(self.instance.stop("stop requested").await) {
                        return Termination::Faulted { detail: e.to_string() };
                    }
                }
                _ = async { match deadline.as_mut().as_pin_mut() { Some(d) => d.await, None => std::future::pending().await } } => {
                    return match self.cancel_world("deadline exceeded").await {
                        Ok(_) => Termination::DeadlineExceeded,
                        Err(e) => Termination::Faulted { detail: e.to_string() },
                    };
                }
                completion = self.completions.recv() => {
                    match completion {
                        Some(c) => {
                            if let Err(e) = self.route(c).await {
                                if self.deadline_elapsed() {
                                    return self.deadline_interrupted().await;
                                }
                                return Termination::Faulted { detail: e.to_string() };
                            }
                        }
                        None => return Termination::Faulted { detail: "completion channel closed".into() },
                    }
                }
            }
        }
    }

    /// The watchdog interrupted synchronous guest work at the deadline: the
    /// world ends as deadline-exceeded (its operations are released like a
    /// cancellation; the guest cannot be asked to unwind after a trap).
    async fn deadline_interrupted(&mut self) -> Termination {
        self.shared.accepting_ops.store(false, Ordering::SeqCst);
        self.cancel.cancel();
        self.ledger.cancel_world(self.id);
        Termination::DeadlineExceeded
    }

    /// Logical cancellation: the world loses interest in its operations, the
    /// guest is told so it can unwind, and nothing is assumed about the
    /// physical operations, which keep their owners.
    async fn cancel_world(&mut self, reason: &str) -> Result<Termination, EngineError> {
        self.shared.accepting_ops.store(false, Ordering::SeqCst);
        self.cancel.cancel();
        self.ledger.cancel_world(self.id);
        let watchdog = self.watchdog();
        watchdog.finish(self.instance.cancel(reason).await)?;
        Ok(Termination::Cancelled {
            reason: reason.to_owned(),
        })
    }

    /// The one teardown path. Never enters the guest.
    fn retire(&mut self, reason: &str) {
        if self.finished {
            return;
        }
        self.finished = true;
        self.shared.accepting_ops.store(false, Ordering::SeqCst);
        self.cancel.cancel();
        let cancelled = self.ledger.cancel_world(self.id);
        // Drain anything already queued so the guards release now rather
        // than when the channel is dropped a moment later.
        self.completions.close();
        while let Ok(c) = self.completions.try_recv() {
            drop(c);
        }
        dec(&self.gauges.live_worlds);
        tracing::debug!(world = %self.id, reason, outstanding = cancelled.len(), "world retired");
    }
}

/// Interrupts the guest if one synchronous run exceeds its slice.
///
/// Guest execution blocks the thread it runs on, so the watchdog cannot be a
/// task on the same executor. One persistent thread serves every world by
/// ticking every few milliseconds over the registered slots; arming and
/// disarming are atomic stores (no lock, no wake-up, no context switch on
/// the request path — the previous heap + condvar design cost ~4 context
/// switches per request). Precision is the tick, which is far below any
/// sensible slice.
struct Watchdog {
    slot: Arc<WatchSlot>,
    slice: Duration,
    cpu_start_ns: u64,
}

/// One world's watch: the deadline the guest must return by (0 = disarmed)
/// as nanoseconds since the service's epoch, and the engine's interrupt flag.
struct WatchSlot {
    deadline_ns: std::sync::atomic::AtomicU64,
    flag: Arc<AtomicBool>,
    /// CPU accounting rides on the same guard: thread CPU time at arm,
    /// delta added at finish.
    cpu_ns: std::sync::atomic::AtomicU64,
}

/// CPU time of the calling thread (CLOCK_THREAD_CPUTIME_ID); a vDSO read.
fn thread_cpu_ns() -> u64 {
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: a valid pointer to a timespec; the clock id is a constant.
    if unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut ts) } != 0 {
        return 0;
    }
    ts.tv_sec as u64 * 1_000_000_000 + ts.tv_nsec as u64
}

const WATCHDOG_TICK: Duration = Duration::from_millis(5);

struct WatchdogService {
    epoch: Instant,
    slots: Mutex<Vec<std::sync::Weak<WatchSlot>>>,
}

fn watchdog_service() -> &'static WatchdogService {
    static SERVICE: std::sync::OnceLock<&'static WatchdogService> = std::sync::OnceLock::new();
    SERVICE.get_or_init(|| {
        let service: &'static WatchdogService = Box::leak(Box::new(WatchdogService {
            epoch: Instant::now(),
            slots: Mutex::new(Vec::new()),
        }));
        std::thread::Builder::new()
            .name("usai-watchdog".into())
            .spawn(move || {
                loop {
                    // Nothing to enforce while no world is live: park instead
                    // of waking 200 times a second in an idle process.
                    crate::idle::wait_until_active();
                    std::thread::sleep(WATCHDOG_TICK);
                    let now = service.epoch.elapsed().as_nanos() as u64;
                    let mut slots = service.slots.lock().expect("watchdog poisoned");
                    // Drop slots whose world is gone; fire the ones past due.
                    slots.retain(|weak| match weak.upgrade() {
                        None => false,
                        Some(slot) => {
                            let deadline = slot.deadline_ns.load(Ordering::Acquire);
                            if deadline != 0 && now >= deadline {
                                slot.flag.store(true, Ordering::SeqCst);
                            }
                            true
                        }
                    });
                }
            })
            .expect("watchdog thread");
        service
    })
}

impl Drop for WatchSlot {
    fn drop(&mut self) {
        crate::idle::leave();
    }
}

impl WatchSlot {
    /// Registers a slot for a world (one lock per world, not per guest call).
    fn register(flag: Arc<AtomicBool>) -> Arc<Self> {
        let slot = Arc::new(Self {
            deadline_ns: std::sync::atomic::AtomicU64::new(0),
            flag,
            cpu_ns: std::sync::atomic::AtomicU64::new(0),
        });
        let service = watchdog_service();
        service
            .slots
            .lock()
            .expect("watchdog poisoned")
            .push(Arc::downgrade(&slot));
        // The tickers run while this slot lives (see `Drop`).
        crate::idle::enter();
        slot
    }
}

impl Watchdog {
    fn arm(slot: &Arc<WatchSlot>, slice: Duration) -> Self {
        slot.flag.store(false, Ordering::SeqCst);
        let deadline = (watchdog_service().epoch.elapsed() + slice).as_nanos() as u64;
        slot.deadline_ns.store(deadline.max(1), Ordering::Release);
        Self {
            slot: Arc::clone(slot),
            slice,
            cpu_start_ns: thread_cpu_ns(),
        }
    }

    fn finish<T>(self, result: Result<T, EngineError>) -> Result<T, EngineError> {
        self.slot.deadline_ns.store(0, Ordering::Release);
        self.slot.cpu_ns.fetch_add(
            thread_cpu_ns().saturating_sub(self.cpu_start_ns),
            Ordering::Relaxed,
        );
        if self.slot.flag.load(Ordering::SeqCst) {
            return Err(EngineError::Guest(format!(
                "guest exceeded the synchronous CPU slice of {:?}",
                self.slice
            )));
        }
        result
    }
}

impl Drop for Watchdog {
    fn drop(&mut self) {
        // `finish` consumes the guard on the normal path; this is the
        // unwinding path, where accounting is best effort.
        self.slot.deadline_ns.store(0, Ordering::Release);
    }
}

impl Drop for WorldDriver {
    fn drop(&mut self) {
        self.retire("dropped without finishing");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn watchdog_sets_the_flag_after_the_slice() {
        let flag = Arc::new(AtomicBool::new(false));
        let slot = WatchSlot::register(Arc::clone(&flag));
        let w = Watchdog::arm(&slot, Duration::from_millis(50));
        std::thread::sleep(Duration::from_millis(120));
        assert!(flag.load(Ordering::SeqCst));
        assert!(w.finish(Ok(())).is_err());
    }

    #[test]
    fn disarmed_watchdog_never_fires() {
        let flag = Arc::new(AtomicBool::new(false));
        let slot = WatchSlot::register(Arc::clone(&flag));
        let w = Watchdog::arm(&slot, Duration::from_millis(30));
        drop(w);
        std::thread::sleep(Duration::from_millis(80));
        assert!(!flag.load(Ordering::SeqCst));
    }

    #[test]
    fn re_armed_watchdog_measures_each_run_separately() {
        let flag = Arc::new(AtomicBool::new(false));
        let slot = WatchSlot::register(Arc::clone(&flag));
        for _ in 0..5 {
            let w = Watchdog::arm(&slot, Duration::from_millis(40));
            std::thread::sleep(Duration::from_millis(15));
            assert!(w.finish(Ok(())).is_ok());
        }
    }
}
