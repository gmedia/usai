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
use crate::runtime::{Revision, Runtime};
use crate::world::Termination;

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
            let Trigger::Service { restart } = &workload.trigger else {
                continue;
            };
            let restart = restart.clone();
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
                loop {
                    let Some(runtime) = runtime.upgrade() else {
                        return;
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
                    let result = runtime
                        .execute_with_stop(
                            admission,
                            input,
                            sup.cancel.child_token(),
                            Some(sup.stop.child_token()),
                        )
                        .await;
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
                    tracing::warn!(service = %name, restarts, delay_ms = delay.as_millis() as u64, "service restarting");
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
