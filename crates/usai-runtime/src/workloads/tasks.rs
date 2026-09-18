//! Tasks: finite work with an independent lifecycle (`GOAL.md` §16–§17,
//! contracts C3 and C15).
//!
//! ```text
//! ctx.tasks.invoke(task, input)    owned: the parent world awaits a child world
//! ctx.tasks.dispatch(task, input)  transferred: the task runtime owns the child;
//!                                  the parent may finish. Non-durable (ADR-0010).
//! ```

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Weak};

use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::admission::Budget;
use crate::host_ops::{ChildRecord, ChildRelation, OpContext, OpFuture, OpHandler, OpOutcome};
use crate::runtime::{Revision, Runtime, RuntimeError};
use crate::world::Termination;

#[derive(Deserialize)]
struct TaskRequest {
    name: String,
    #[serde(default)]
    input: Value,
}

fn task_id(name: &str) -> String {
    format!("task:{name}")
}

/// Outcome of a task world, encoded for the parent's promise.
fn outcome_for_parent(result: &crate::world::WorkResult) -> OpOutcome {
    match (&result.termination, &result.outcome) {
        (Termination::Completed, Some(Ok(value))) => {
            OpOutcome::ok(value.get("value").unwrap_or(&Value::Null))
        }
        (Termination::Completed, Some(Err(error))) => {
            let (code, status) = error
                .usai
                .as_ref()
                .map(|u| {
                    (
                        u.get("code")
                            .and_then(Value::as_str)
                            .unwrap_or("task_failed")
                            .to_owned(),
                        u.get("status").and_then(Value::as_u64).unwrap_or(500) as u16,
                    )
                })
                .unwrap_or_else(|| ("task_failed".into(), 500));
            OpOutcome::err(&code, status, error.message.clone())
        }
        (Termination::DeadlineExceeded, _) => OpOutcome::err(
            "task_deadline_exceeded",
            504,
            "the task did not complete within its deadline",
        ),
        (Termination::Cancelled { reason }, _) => {
            OpOutcome::err("cancelled", 499, format!("task cancelled: {reason}"))
        }
        (Termination::Faulted { detail }, _) => OpOutcome::err("task_faulted", 500, detail.clone()),
        (Termination::Completed, None) => {
            OpOutcome::err("task_no_outcome", 500, "the task produced no outcome")
        }
    }
}

/// `task.invoke`: the parent owns the child. The child world is cancelled
/// with the parent (its token descends from the operation's).
pub struct InvokeHandler {
    runtime: Weak<Runtime>,
}

impl OpHandler for InvokeHandler {
    fn start(&self, ctx: OpContext, payload: String) -> Result<OpFuture, OpOutcome> {
        let request: TaskRequest = serde_json::from_str(&payload)
            .map_err(|e| OpOutcome::err("invalid_task_request", 500, e.to_string()))?;
        let runtime = self
            .runtime
            .upgrade()
            .ok_or_else(|| OpOutcome::err("runtime_gone", 503, "runtime is shutting down"))?;
        let revision = ctx
            .revision
            .clone()
            .ok_or_else(|| OpOutcome::err("no_revision", 500, "world has no revision"))?;
        let id = task_id(&request.name);
        let admission = runtime
            .admit_in_flight(&revision, &id)
            .map_err(|e| match e {
                RuntimeError::UnknownWorkload(_) => OpOutcome::err(
                    "unknown_task",
                    500,
                    format!("no task named {}", request.name),
                ),
                RuntimeError::Admission(_) => {
                    OpOutcome::err("capacity_exhausted", 503, e.to_string())
                }
                other => OpOutcome::err("task_admission_failed", 500, other.to_string()),
            })?;
        let child_id = format!("{}#{}", id, ctx.op);
        ctx.children
            .lock()
            .expect("children poisoned")
            .push(ChildRecord {
                workload: id.clone(),
                relation: ChildRelation::Owned,
                id: child_id,
            });
        let input = super::input(&revision, "task", json!({ "input": request.input }));
        let cancel = ctx.cancel.child_token();
        Ok(Box::pin(async move {
            match runtime.execute(admission, input, cancel).await {
                Ok(result) => outcome_for_parent(&result),
                Err(e) => OpOutcome::err("task_execution_failed", 500, e.to_string()),
            }
        }))
    }
}

/// `task.dispatch`: ownership transfers to the task runtime. The parent's
/// promise resolves as soon as the task is queued.
pub struct DispatchHandler {
    queue: Arc<TaskQueue>,
}

impl OpHandler for DispatchHandler {
    fn start(&self, ctx: OpContext, payload: String) -> Result<OpFuture, OpOutcome> {
        let request: TaskRequest = serde_json::from_str(&payload)
            .map_err(|e| OpOutcome::err("invalid_task_request", 500, e.to_string()))?;
        let revision = ctx
            .revision
            .clone()
            .ok_or_else(|| OpOutcome::err("no_revision", 500, "world has no revision"))?;
        let id = task_id(&request.name);
        if revision.definition.workload(&id).is_none() {
            return Err(OpOutcome::err(
                "unknown_task",
                500,
                format!("no task named {}", request.name),
            ));
        }
        let dispatch_id = self.queue.enqueue(Dispatched {
            revision: Arc::clone(&revision),
            workload: id.clone(),
            input: super::input(&revision, "task", json!({ "input": request.input })),
            parent: ctx.world,
        })?;
        ctx.children
            .lock()
            .expect("children poisoned")
            .push(ChildRecord {
                workload: id,
                relation: ChildRelation::Transferred,
                id: dispatch_id.clone(),
            });
        Ok(Box::pin(async move {
            OpOutcome::ok(&json!({ "id": dispatch_id }))
        }))
    }
}

pub struct Dispatched {
    pub revision: Arc<Revision>,
    pub workload: String,
    pub input: Value,
    pub parent: crate::ownership::WorldId,
}

/// Runtime-owned, bounded, non-durable task queue. Its budget bounds how
/// many transferred tasks run at once; its channel bounds how many wait.
pub struct TaskQueue {
    tx: mpsc::Sender<(String, Dispatched)>,
    budget: Arc<Budget>,
    next: AtomicU64,
    pub queued: AtomicU64,
    pub completed: AtomicU64,
    pub failed: AtomicU64,
    pub lost: AtomicU64,
}

impl TaskQueue {
    pub fn start(
        runtime: Weak<Runtime>,
        capacity: usize,
        concurrency: u32,
        shutdown: CancellationToken,
    ) -> Arc<Self> {
        let (tx, mut rx) = mpsc::channel::<(String, Dispatched)>(capacity);
        let queue = Arc::new(Self {
            tx,
            budget: Budget::new("runtime.tasks", concurrency),
            next: AtomicU64::new(1),
            queued: AtomicU64::new(0),
            completed: AtomicU64::new(0),
            failed: AtomicU64::new(0),
            lost: AtomicU64::new(0),
        });
        let worker = Arc::clone(&queue);
        tokio::spawn(async move {
            loop {
                let (id, dispatched) = tokio::select! {
                    next = rx.recv() => match next { Some(n) => n, None => break },
                    _ = shutdown.cancelled() => break,
                };
                worker.queued.fetch_sub(1, Ordering::SeqCst);
                let Some(runtime) = runtime.upgrade() else {
                    break;
                };
                // Bounded wait: transferred work queues rather than being
                // refused, because its owner is the runtime, not a client.
                let permit = worker.budget.acquire().await;
                let queue = Arc::clone(&worker);
                let cancel = shutdown.child_token();
                tokio::spawn(async move {
                    let _permit = permit;
                    let admitted =
                        runtime.admit_in_flight(&dispatched.revision, &dispatched.workload);
                    // The queue's hold on the revision ends here; the
                    // admission's own in-flight count takes over.
                    dispatched.revision.release_child();
                    let admission = match admitted {
                        Ok(a) => a,
                        Err(e) => {
                            queue.failed.fetch_add(1, Ordering::SeqCst);
                            tracing::error!(task = %id, error = %e, "dispatched task could not be admitted");
                            return;
                        }
                    };
                    match runtime.execute(admission, dispatched.input, cancel).await {
                        Ok(result) => match (&result.termination, &result.outcome) {
                            (Termination::Completed, Some(Ok(_))) => {
                                queue.completed.fetch_add(1, Ordering::SeqCst);
                                tracing::debug!(task = %id, world = %result.world, parent = %dispatched.parent, "task completed");
                            }
                            (termination, outcome) => {
                                queue.failed.fetch_add(1, Ordering::SeqCst);
                                let termination = serde_json::to_value(termination)
                                    .ok()
                                    .and_then(|v| {
                                        v.as_str().map(str::to_owned).or_else(|| {
                                            v.get("detail")
                                                .and_then(|d| d.as_str())
                                                .map(str::to_owned)
                                        })
                                    })
                                    .unwrap_or_else(|| "faulted".to_owned());
                                let error = outcome
                                    .as_ref()
                                    .and_then(|o| o.as_ref().err().map(|e| e.message.clone()))
                                    .unwrap_or_else(|| "no outcome".to_owned());
                                tracing::warn!(task = %id, world = %result.world, %termination, %error, "task failed");
                            }
                        },
                        Err(e) => {
                            queue.failed.fetch_add(1, Ordering::SeqCst);
                            tracing::error!(task = %id, error = %e, "task execution failed");
                        }
                    }
                });
            }
            // Anything still queued at shutdown is lost: non-durable by
            // contract (ADR-0010). Count it so the loss is visible.
            while let Ok((id, _)) = rx.try_recv() {
                worker.lost.fetch_add(1, Ordering::SeqCst);
                tracing::warn!(task = %id, "dispatched task lost at shutdown (local dispatch is not durable)");
            }
        });
        queue
    }

    fn enqueue(&self, dispatched: Dispatched) -> Result<String, OpOutcome> {
        let id = format!(
            "{}#d{}",
            dispatched.workload,
            self.next.fetch_add(1, Ordering::SeqCst)
        );
        // Draining a revision must wait for transferred work it still owns.
        dispatched.revision.retain_for_child();
        match self.tx.try_send((id.clone(), dispatched)) {
            Ok(()) => {
                self.queued.fetch_add(1, Ordering::SeqCst);
                Ok(id)
            }
            Err(mpsc::error::TrySendError::Full((_, d))) => {
                d.revision.release_child();
                Err(OpOutcome::err(
                    "task_queue_full",
                    503,
                    "the task queue is full",
                ))
            }
            Err(mpsc::error::TrySendError::Closed((_, d))) => {
                d.revision.release_child();
                Err(OpOutcome::err(
                    "runtime_gone",
                    503,
                    "runtime is shutting down",
                ))
            }
        }
    }

    pub fn status(&self) -> Value {
        json!({
            "queued": self.queued.load(Ordering::SeqCst),
            "running": self.budget.in_use(),
            "max": self.budget.max(),
            "completed": self.completed.load(Ordering::SeqCst),
            "failed": self.failed.load(Ordering::SeqCst),
            "lost": self.lost.load(Ordering::SeqCst),
        })
    }
}

pub fn handlers(
    runtime: Weak<Runtime>,
    queue: Arc<TaskQueue>,
) -> Vec<(&'static str, Arc<dyn OpHandler>)> {
    vec![
        ("task.invoke", Arc::new(InvokeHandler { runtime })),
        ("task.dispatch", Arc::new(DispatchHandler { queue })),
    ]
}
