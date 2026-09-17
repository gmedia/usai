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
    Compiled, Engine, EngineError, GuestError, HostBindings, Outcome, WorldInstance,
};
use crate::host_ops::{Completion, OpContext, OpExtensions, spawn_operation};
use crate::ownership::{Gauges, Ledger, OpId, WorldId, dec, inc};
use crate::resource::BoundResources;

/// A lifecycle rule the world violated. The message teaches the model
/// (`GOAL.md` §45); the code is stable for tooling.
#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LifecycleViolation {
    pub code: &'static str,
    pub message: String,
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
        Self {
            code: "detached_work",
            message: format!(
                "{kind} work `{}` ended with live asynchronous work ({live}).\n\n\
                 The {kind} lifetime ended when its result was produced. Work that is still \
                 pending cannot remain owned by this world, so it was cancelled.\n\n\
                 Use:\n  \
                 task()    for independent finite work (ctx.tasks.dispatch)\n  \
                 cron()    for scheduled work\n  \
                 service() for intentional long-running work\n\
                 or await the work before returning.",
                workload.name
            ),
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
    logs: Mutex<Vec<LogLine>>,
    accepting_ops: AtomicBool,
    max_logs: usize,
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
        tracing::debug!(world = %self.id, level, "{message}");
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
    delivered: u32,
    dropped: u32,
    finished: bool,
}

impl WorldDriver {
    pub async fn create(
        engine: &dyn Engine,
        ledger: Arc<Ledger>,
        spec: WorldSpec,
    ) -> Result<Self, EngineError> {
        let id = ledger.next_world_id();
        let (tx, rx) = mpsc::channel(64);
        let shared = Arc::new(WorldShared {
            id,
            ledger: Arc::clone(&ledger),
            cancel: spec.cancel.clone(),
            resources: spec.resources,
            extensions: spec.extensions,
            completions: tx,
            logs: Mutex::new(Vec::new()),
            accepting_ops: AtomicBool::new(true),
            max_logs: 1_000,
        });
        let bindings: Arc<dyn HostBindings> = Arc::clone(&shared) as Arc<dyn HostBindings>;
        let instance = engine.instantiate(&spec.compiled, bindings).await?;
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
            delivered: 0,
            dropped: 0,
            finished: false,
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

    /// Arms the CPU-slice watchdog for one guest entry.
    fn watchdog(&self) -> Watchdog {
        Watchdog::arm(self.instance.interrupter(), self.cpu_slice)
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
            Err(e) => Termination::Faulted {
                detail: e.to_string(),
            },
        };

        let outcome = match self.instance.outcome().await {
            Ok(Some(Outcome::Ok { value, .. })) => Some(Ok(value)),
            Ok(Some(Outcome::Err { error, .. })) => Some(Err(error)),
            Ok(None) => None,
            Err(_) => None,
        };

        let mut violations = Vec::new();
        if matches!(termination, Termination::Completed)
            && self.workload().lifetime() == LifetimeFamily::Finite
            && let Ok(pending) = self.instance.pending().await
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
        let result = WorkResult {
            world: self.id,
            workload: workload_id,
            termination,
            outcome,
            violations,
            duration: started.elapsed(),
            completions_delivered: self.delivered,
            completions_dropped: self.dropped,
            logs,
        };
        self.retire("finished");
        result
    }

    async fn drive(&mut self) -> Termination {
        let deadline = self.deadline.map(|d| tokio::time::sleep(d));
        tokio::pin!(deadline);
        loop {
            match self.instance.outcome().await {
                Ok(Some(_)) => return Termination::Completed,
                Ok(None) => {}
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
                                return Termination::Faulted { detail: e.to_string() };
                            }
                        }
                        None => return Termination::Faulted { detail: "completion channel closed".into() },
                    }
                }
            }
        }
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
/// task on the same executor. One persistent thread serves every world; its
/// queue is bounded by (arm rate x slice) because disarmed entries are
/// discarded as their deadlines pass.
struct Watchdog {
    armed: Arc<AtomicBool>,
    flag: Arc<AtomicBool>,
    slice: Duration,
}

struct WatchdogEntry {
    deadline: Instant,
    armed: Arc<AtomicBool>,
    flag: Arc<AtomicBool>,
}

impl PartialEq for WatchdogEntry {
    fn eq(&self, other: &Self) -> bool {
        self.deadline == other.deadline
    }
}
impl Eq for WatchdogEntry {}
impl PartialOrd for WatchdogEntry {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for WatchdogEntry {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // BinaryHeap is a max-heap; the earliest deadline must come out first.
        other.deadline.cmp(&self.deadline)
    }
}

struct WatchdogService {
    queue: Mutex<std::collections::BinaryHeap<WatchdogEntry>>,
    wake: std::sync::Condvar,
}

fn watchdog_service() -> &'static WatchdogService {
    static SERVICE: std::sync::OnceLock<&'static WatchdogService> = std::sync::OnceLock::new();
    SERVICE.get_or_init(|| {
        let service: &'static WatchdogService = Box::leak(Box::new(WatchdogService {
            queue: Mutex::new(std::collections::BinaryHeap::new()),
            wake: std::sync::Condvar::new(),
        }));
        std::thread::Builder::new()
            .name("usai-watchdog".into())
            .spawn(move || {
                let mut queue = service.queue.lock().expect("watchdog poisoned");
                loop {
                    let now = Instant::now();
                    while let Some(next) = queue.peek() {
                        if next.deadline > now {
                            break;
                        }
                        let entry = queue.pop().expect("peeked");
                        if entry.armed.load(Ordering::SeqCst) {
                            entry.flag.store(true, Ordering::SeqCst);
                        }
                    }
                    let wait = queue
                        .peek()
                        .map(|e| e.deadline.saturating_duration_since(now));
                    queue = match wait {
                        Some(wait) => {
                            service
                                .wake
                                .wait_timeout(queue, wait)
                                .expect("watchdog poisoned")
                                .0
                        }
                        None => service.wake.wait(queue).expect("watchdog poisoned"),
                    };
                }
            })
            .expect("watchdog thread");
        service
    })
}

impl Watchdog {
    fn arm(flag: Arc<AtomicBool>, slice: Duration) -> Self {
        flag.store(false, Ordering::SeqCst);
        let armed = Arc::new(AtomicBool::new(true));
        let service = watchdog_service();
        service
            .queue
            .lock()
            .expect("watchdog poisoned")
            .push(WatchdogEntry {
                deadline: Instant::now() + slice,
                armed: Arc::clone(&armed),
                flag: Arc::clone(&flag),
            });
        service.wake.notify_one();
        Self { armed, flag, slice }
    }

    fn finish<T>(self, result: Result<T, EngineError>) -> Result<T, EngineError> {
        self.armed.store(false, Ordering::SeqCst);
        if self.flag.load(Ordering::SeqCst) {
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
        self.armed.store(false, Ordering::SeqCst);
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
        let w = Watchdog::arm(Arc::clone(&flag), Duration::from_millis(50));
        std::thread::sleep(Duration::from_millis(120));
        assert!(flag.load(Ordering::SeqCst));
        assert!(w.finish(Ok(())).is_err());
    }

    #[test]
    fn disarmed_watchdog_never_fires() {
        let flag = Arc::new(AtomicBool::new(false));
        let w = Watchdog::arm(Arc::clone(&flag), Duration::from_millis(30));
        drop(w);
        std::thread::sleep(Duration::from_millis(80));
        assert!(!flag.load(Ordering::SeqCst));
    }
}
