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
            if !matches!(workload.trigger, Trigger::Service) {
                continue;
            }
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
                });
            let sup = Arc::clone(&supervisor);
            let runtime = runtime.clone();
            let revision = Arc::clone(revision);
            let id = workload.id.clone();
            let name = workload.name.clone();
            let handle = tokio::spawn(async move {
                let Some(runtime) = runtime.upgrade() else {
                    return;
                };
                let admission = match runtime.admit_child(&revision, &id) {
                    Ok(a) => a,
                    Err(e) => {
                        sup.set(index, ServiceState::Failed, None, Some(e.to_string()));
                        tracing::error!(service = %name, error = %e, "service could not be admitted");
                        return;
                    }
                };
                let input = super::input(&revision, "service", json!({}));
                sup.set(index, ServiceState::Running, None, None);
                tracing::info!(service = %name, "service starting");
                let result = runtime
                    .execute_with_stop(
                        admission,
                        input,
                        sup.cancel.child_token(),
                        Some(sup.stop.child_token()),
                    )
                    .await;
                match result {
                    Ok(r) => {
                        let (state, error) = match (&r.termination, &r.outcome) {
                            (Termination::Completed, Some(Ok(_))) => (ServiceState::Stopped, None),
                            (Termination::Completed, Some(Err(e))) => (
                                ServiceState::Failed,
                                Some(format!("{}: {}", e.name, e.message)),
                            ),
                            (Termination::Cancelled { .. }, _) => (ServiceState::Stopped, None),
                            (t, _) => (ServiceState::Failed, Some(format!("{t:?}"))),
                        };
                        if state == ServiceState::Failed {
                            tracing::error!(service = %name, world = %r.world, error = ?error, "service ended with failure");
                        } else {
                            tracing::info!(service = %name, world = %r.world, "service stopped");
                        }
                        sup.set(index, state, Some(r.world), error);
                    }
                    Err(e) => {
                        sup.set(index, ServiceState::Failed, None, Some(e.to_string()));
                        tracing::error!(service = %name, error = %e, "service world failed");
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
                restarts: self.restarts.load(Ordering::SeqCst),
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
