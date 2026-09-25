//! Services: intentional long-running work (`GOAL.md` §22, contract C2).
//!
//! A service world starts when its revision activates and lives until the
//! revision drains. Its mutable state persists because the service is
//! alive — never because the process is. Stop is graceful first (the
//! signal fires, `ctx.sleep` returns, the loop exits), hard after the
//! drain timeout.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use serde::Serialize;
use serde_json::json;
use tokio_util::sync::CancellationToken;

use crate::definition::Trigger;
use crate::ownership::WorldId;
use crate::resource::ResourceManager;
use crate::runtime::{Revision, Runtime};
use crate::world::Termination;

/// The ledger an `exclusive` service claims itself in: one row per service
/// name, held by one instance at a time.
///
/// A row with a deadline rather than a held connection or an advisory lock:
/// a world never holds a manager here, every operation leases a connection
/// and gives it back (`docs/LIFECYCLE-CONTRACTS.md` C5), and a lease that
/// outlives its holder's crash by a stated amount is easier to reason about
/// than one that depends on when a TCP connection is noticed to be gone.
pub const LEASES_SCHEMA: &str = "CREATE TABLE IF NOT EXISTS usai_service_leases (
  name text PRIMARY KEY,
  holder text NOT NULL,
  expires_at timestamptz NOT NULL,
  claimed_at timestamptz NOT NULL DEFAULT now()
)";

/// Takes or renews the lease on `name`, returning whether this instance
/// holds it. Free, expired, or already ours — otherwise somebody else's.
///
/// One statement, so two instances racing cannot both win: the `WHERE` on
/// the conflicting row is what decides, inside the same transaction as the
/// insert.
pub async fn claim_lease(
    manager: &dyn ResourceManager,
    name: &str,
    holder: &str,
    lease_ms: u64,
) -> Result<bool, String> {
    const CLAIM: &str = "INSERT INTO usai_service_leases (name, holder, expires_at) \
         VALUES ($1, $2, now() + ($3::bigint * interval '1 millisecond')) \
         ON CONFLICT (name) DO UPDATE SET holder = EXCLUDED.holder, \
           expires_at = EXCLUDED.expires_at, claimed_at = now() \
         WHERE usai_service_leases.holder = EXCLUDED.holder \
            OR usai_service_leases.expires_at < now()";
    let params = || {
        vec![
            json!(name),
            json!(holder),
            json!(i64::try_from(lease_ms).unwrap_or(30_000)),
        ]
    };
    let taken = super::queue::sql(manager, "execute", CLAIM, params()).await;
    let taken = match taken {
        Ok(n) => n.as_u64().unwrap_or(0) == 1,
        // A fresh database has no table yet: prepare it once and claim
        // again, the way the cron ledger and the queue do.
        Err(e) if e.contains("usai_service_leases") || e.contains("42P01") => {
            let _ = super::queue::sql(manager, "execute", LEASES_SCHEMA, vec![]).await;
            match super::queue::sql(manager, "execute", CLAIM, params()).await {
                Ok(n) => n.as_u64().unwrap_or(0) == 1,
                Err(e) => return Err(e),
            }
        }
        Err(e) => return Err(e),
    };
    Ok(taken)
}

/// Gives the lease up so another instance can start at once instead of
/// waiting out the deadline. Best effort: a drain that cannot reach the
/// database still ends, and the lease expires on its own.
pub async fn release_lease(manager: &dyn ResourceManager, name: &str, holder: &str) {
    let _ = super::queue::sql(
        manager,
        "execute",
        "DELETE FROM usai_service_leases WHERE name = $1 AND holder = $2",
        vec![json!(name), json!(holder)],
    )
    .await;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ServiceState {
    Starting,
    Running,
    Stopping,
    Stopped,
    /// Ended, and the restart policy is waiting out its backoff. Published
    /// for the whole delay — `failed` used to be, so an alert written from
    /// the runbook fired on every ordinary restart and a health check built
    /// on it took a healthy instance out of rotation for two minutes.
    Restarting,
    /// The supervisor gave up: the restart policy is exhausted, or the
    /// service failed under a policy that does not restart. It stays this
    /// way until the next activation.
    Failed,
    /// `exclusive: true`, and another instance holds the lease. This one is
    /// healthy and ready to take over; it is not `stopped` (it did not end)
    /// and not `failed` (nothing is wrong), and on a two-replica deployment
    /// it is the ordinary state of one of them.
    Waiting,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceStatus {
    pub name: String,
    pub state: ServiceState,
    pub world: Option<WorldId>,
    pub restarts: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

struct ServiceEntry {
    name: String,
    state: ServiceState,
    world: Option<WorldId>,
    last_error: Option<String>,
    /// This service's own restarts. It used to be one counter for the whole
    /// supervisor, reported per service — so a service that had never
    /// failed showed its neighbour's count.
    restarts: u64,
}

/// One supervisor per revision. Owns the stop token every service world
/// listens to, and the join handles drain waits on.
pub struct Supervisor {
    stop: CancellationToken,
    cancel: CancellationToken,
    entries: Mutex<Vec<ServiceEntry>>,
    handles: Mutex<Vec<tokio::task::JoinHandle<()>>>,
    restarts: AtomicU64,
}

impl Supervisor {
    pub fn start(
        runtime: Weak<Runtime>,
        revision: &Arc<Revision>,
        shutdown: &CancellationToken,
    ) -> Arc<Self> {
        let supervisor = Arc::new(Self {
            stop: CancellationToken::new(),
            cancel: shutdown.child_token(),
            entries: Mutex::new(Vec::new()),
            handles: Mutex::new(Vec::new()),
            restarts: AtomicU64::new(0),
        });
        for workload in revision.definition.workloads() {
            let Trigger::Service {
                restart,
                exclusive,
                database,
                lease_ms,
            } = &workload.trigger
            else {
                continue;
            };
            let restart = restart.clone();
            let exclusive = *exclusive;
            let lease_database = database.clone();
            let lease_ms = lease_ms.unwrap_or(30_000).max(3_000);
            let index = supervisor.entries.lock().expect("services poisoned").len();
            supervisor
                .entries
                .lock()
                .expect("services poisoned")
                .push(ServiceEntry {
                    name: workload.name.clone(),
                    state: ServiceState::Starting,
                    world: None,
                    last_error: None,
                    restarts: 0,
                });
            let sup = Arc::clone(&supervisor);
            let runtime = runtime.clone();
            let revision = Arc::clone(revision);
            let id = workload.id.clone();
            let name = workload.name.clone();
            let handle = tokio::spawn(async move {
                let mut restarts: u32 = 0;
                // Who this instance is in the ledger, the way a cron claim
                // names itself: enough to tell two replicas apart in a log.
                let holder = format!("{}:{}", super::cron::instance_name(), std::process::id());
                loop {
                    let Some(runtime) = runtime.upgrade() else {
                        return;
                    };

                    // Exactly one instance runs an `exclusive` service. The
                    // others wait here — cheaply, and visibly: `waiting` is
                    // a state an operator can see rather than a service that
                    // looks stopped for no reason.
                    let lease = if exclusive {
                        let Some(db) =
                            super::queue::database_for(&revision, lease_database.as_deref())
                        else {
                            sup.set(index, ServiceState::Failed, None, Some(
                                "exclusive: true needs a postgres resource to hold the lease in".into(),
                            ));
                            tracing::error!(service = %name, "exclusive service has no database for its lease");
                            return;
                        };
                        loop {
                            if sup.stop.is_cancelled() || sup.cancel.is_cancelled() {
                                return;
                            }
                            match claim_lease(db.as_ref(), &name, &holder, lease_ms).await {
                                Ok(true) => break,
                                Ok(false) => {
                                    sup.set(index, ServiceState::Waiting, None, None);
                                }
                                // A flaky database must not make every
                                // replica decide it is the one: not holding
                                // the lease is the safe answer, so wait.
                                Err(e) => {
                                    sup.set(index, ServiceState::Waiting, None, Some(e.clone()));
                                    tracing::warn!(service = %name, error = %e, "could not claim the service lease");
                                }
                            }
                            tokio::select! {
                                _ = tokio::time::sleep(Duration::from_millis(lease_ms / 3)) => {}
                                _ = sup.stop.cancelled() => return,
                                _ = sup.cancel.cancelled() => return,
                            }
                        }
                        tracing::info!(service = %name, holder, lease_ms, "service lease held");
                        Some(db)
                    } else {
                        None
                    };
                    let admission = match runtime.admit_in_flight(&revision, &id) {
                        Ok(a) => a,
                        Err(e) => {
                            sup.set(index, ServiceState::Failed, None, Some(e.to_string()));
                            tracing::error!(service = %name, error = %e, "service could not be admitted");
                            return;
                        }
                    };
                    let input = super::input(&revision, "service", json!({}));
                    sup.set(index, ServiceState::Running, None, None);
                    tracing::info!(service = %name, restarts, "service starting");
                    // While the world runs, the lease is renewed every
                    // third of its life. If a renew fails — the row was
                    // taken because this instance stalled past the deadline,
                    // or the database is gone — the world is **stopped**,
                    // not cancelled: an exclusive service is exclusive
                    // because something else must not be writing at the same
                    // time, so losing the claim has to end the work, and it
                    // ends it the way a drain does so `close`-shaped cleanup
                    // still runs.
                    let world_stop = sup.stop.child_token();
                    let renewer = lease.clone().map(|db| {
                        let name = name.clone();
                        let holder = holder.clone();
                        let stop = world_stop.clone();
                        tokio::spawn(async move {
                            loop {
                                tokio::select! {
                                    _ = tokio::time::sleep(Duration::from_millis(lease_ms / 3)) => {}
                                    _ = stop.cancelled() => return,
                                }
                                match claim_lease(db.as_ref(), &name, &holder, lease_ms).await {
                                    Ok(true) => {}
                                    Ok(false) => {
                                        tracing::warn!(service = %name, "service lease lost to another instance; stopping");
                                        stop.cancel();
                                        return;
                                    }
                                    Err(e) => {
                                        tracing::warn!(service = %name, error = %e, "could not renew the service lease; stopping");
                                        stop.cancel();
                                        return;
                                    }
                                }
                            }
                        })
                    });
                    let result = runtime
                        .execute_with_stop(
                            admission,
                            input,
                            sup.cancel.child_token(),
                            Some(world_stop.clone()),
                        )
                        .await;
                    if let Some(renewer) = renewer {
                        renewer.abort();
                    }
                    if let Some(db) = &lease {
                        // Give it up rather than let it expire: the replica
                        // waiting to take over starts now instead of a
                        // lease-time later.
                        release_lease(db.as_ref(), &name, &holder).await;
                    }
                    drop(runtime);
                    let (state, error, world) = match result {
                        Ok(r) => {
                            let (state, error) = match (&r.termination, &r.outcome) {
                                (Termination::Completed, Some(Ok(_))) => {
                                    (ServiceState::Stopped, None)
                                }
                                (Termination::Completed, Some(Err(e))) => (
                                    ServiceState::Failed,
                                    Some(format!("{}: {}", e.name, e.message)),
                                ),
                                (Termination::Cancelled { .. }, _) => (ServiceState::Stopped, None),
                                // The service ran out its **declared**
                                // deadline. Nothing failed: it ended as it
                                // was asked to, and charging that to the
                                // restart policy made a bounded service
                                // dead for good after its first cycle under
                                // the default policy.
                                (Termination::DeadlineExceeded, _) => (ServiceState::Stopped, None),
                                (t, _) => (ServiceState::Failed, Some(format!("{t:?}"))),
                            };
                            (state, error, Some(r.world))
                        }
                        Err(e) => (ServiceState::Failed, Some(e.to_string()), None),
                    };
                    let world_text = world.map(|w| w.to_string()).unwrap_or_default();
                    if state == ServiceState::Failed {
                        tracing::error!(service = %name, world = %world_text, error = error.as_deref().unwrap_or(""), "service ended with failure");
                    } else {
                        tracing::info!(service = %name, world = %world_text, "service stopped");
                    }
                    sup.set(index, state, world, error);
                    // Restart applies only while the revision wants the service
                    // alive; a stop request always wins.
                    let wants_restart = !sup.stop.is_cancelled()
                        && match restart.mode.as_str() {
                            "always" => true,
                            "on-failure" => state == ServiceState::Failed,
                            _ => false,
                        }
                        && (restart.max_restarts == 0 || restarts < restart.max_restarts);
                    if !wants_restart {
                        if state == ServiceState::Failed && !sup.stop.is_cancelled() {
                            // The supervisor gives up here: the state stays
                            // `failed` in /_usai/status and the metrics until
                            // the next activation; readiness is unaffected.
                            // "restart policy exhausted" with `restarts=0`
                            // was incoherent: under `never` there was never
                            // a restart to exhaust.
                            let why = match restart.mode.as_str() {
                                "always" | "on-failure" => "restart policy exhausted",
                                _ => "this service declares no restart policy",
                            };
                            tracing::error!(
                                service = %name,
                                restarts,
                                max_restarts = restart.max_restarts,
                                mode = %restart.mode,
                                "service gave up: {why} (state failed until the next activation; /_usai/ready does not depend on services)"
                            );
                        }
                        return;
                    }
                    restarts += 1;
                    sup.restarts.fetch_add(1, Ordering::SeqCst);
                    sup.bump_restarts(index);
                    // The service is between runs, not gone: `failed` has to
                    // keep meaning "gave up" or nothing can alert on it.
                    sup.set(index, ServiceState::Restarting, None, None);
                    let delay = Duration::from_millis(
                        restart
                            .backoff_ms
                            .saturating_mul(1u64 << (restarts - 1).min(10)),
                    );
                    // `failed` is what an operator alerts on and `restarting`
                    // is what they do not — `docs/runbooks/application-failures.md`
                    // says so, and the state says so. The *line* said the
                    // opposite: a bounded service under `restart: { mode:
                    // "always" }` ends normally and restarts every cycle, so
                    // a healthy loop wrote a WARN every few seconds at the
                    // level people filter on. A restart after a **failure**
                    // is still a WARN; a restart after a normal end is the
                    // policy doing its job.
                    if state == ServiceState::Failed {
                        tracing::warn!(service = %name, restarts, delay_ms = delay.as_millis() as u64, "service restarting after a failure");
                    } else {
                        tracing::info!(service = %name, restarts, delay_ms = delay.as_millis() as u64, "service restarting");
                    }
                    tokio::select! {
                        _ = tokio::time::sleep(delay) => {}
                        _ = sup.stop.cancelled() => return,
                    }
                }
            });
            supervisor
                .handles
                .lock()
                .expect("services poisoned")
                .push(handle);
        }
        supervisor
    }

    fn bump_restarts(&self, index: usize) {
        let mut entries = self.entries.lock().expect("services poisoned");
        if let Some(entry) = entries.get_mut(index) {
            entry.restarts += 1;
        }
    }

    fn set(
        &self,
        index: usize,
        state: ServiceState,
        world: Option<WorldId>,
        error: Option<String>,
    ) {
        let mut entries = self.entries.lock().expect("services poisoned");
        if let Some(entry) = entries.get_mut(index) {
            entry.state = state;
            if world.is_some() {
                entry.world = world;
            }
            entry.last_error = error;
        }
    }

    pub fn status(&self) -> Vec<ServiceStatus> {
        self.entries
            .lock()
            .expect("services poisoned")
            .iter()
            .map(|e| ServiceStatus {
                name: e.name.clone(),
                state: e.state,
                world: e.world,
                restarts: e.restarts,
                last_error: e.last_error.clone(),
            })
            .collect()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.lock().expect("services poisoned").is_empty()
    }

    /// Graceful stop, then hard cancel after `grace`. Returns once every
    /// service task has ended.
    pub async fn stop(&self, grace: Duration) {
        {
            let mut entries = self.entries.lock().expect("services poisoned");
            for e in entries.iter_mut() {
                if e.state == ServiceState::Running {
                    e.state = ServiceState::Stopping;
                }
            }
        }
        self.stop.cancel();
        let handles: Vec<_> = std::mem::take(&mut *self.handles.lock().expect("services poisoned"));
        let wait = async {
            for h in handles {
                let _ = h.await;
            }
        };
        if tokio::time::timeout(grace, wait).await.is_err() {
            tracing::warn!("services did not stop within {grace:?}; cancelling");
            self.cancel.cancel();
        }
    }
}
