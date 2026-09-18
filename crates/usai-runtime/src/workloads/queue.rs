//! Queue / message workload (`GOAL.md` §19, contracts C2, C16).
//!
//! ```text
//! persistent consumer (revision lifetime)
//!     ├── message A → fresh world → done | retry | dead
//!     ├── message B → fresh world → …
//!     └── message C → fresh world → …
//! ```
//!
//! The v0 substrate is a PostgreSQL table claimed with
//! `FOR UPDATE SKIP LOCKED`: at-least-once delivery, explicit retry with
//! backoff, a dead-letter state. No broker dependency; the delivery
//! guarantee is the table's, and it is stated here (ADR-0014).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Weak};
use std::time::Duration;

use serde::Deserialize;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use crate::definition::Trigger;
use crate::host_ops::{OpContext, OpFuture, OpHandler, OpOutcome};
use crate::resource::{ResourceCall, ResourceManager};
use crate::runtime::{Revision, Runtime};
use crate::world::Termination;

const POLL_INTERVAL: Duration = Duration::from_millis(250);

pub const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS usai_queue (
  id bigserial PRIMARY KEY,
  topic text NOT NULL,
  payload jsonb NOT NULL,
  state text NOT NULL DEFAULT 'ready',
  attempts int NOT NULL DEFAULT 0,
  available_at timestamptz NOT NULL DEFAULT now(),
  locked_at timestamptz,
  locked_by text,
  last_error text,
  created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS usai_queue_ready ON usai_queue (topic, available_at) WHERE state = 'ready';";

#[derive(Default)]
pub struct QueueStats {
    pub claimed: AtomicU64,
    pub done: AtomicU64,
    pub retried: AtomicU64,
    pub dead: AtomicU64,
    pub invalid: AtomicU64,
}

fn database_for(revision: &Revision, name: Option<&str>) -> Option<Arc<dyn ResourceManager>> {
    let resources = revision.resources();
    let name = match name {
        Some(n) => n.to_owned(),
        None => revision
            .definition
            .resources()
            .iter()
            .find(|r| r.kind == "postgres")?
            .name
            .clone(),
    };
    resources.get(&name).cloned()
}

async fn sql(
    manager: &dyn ResourceManager,
    method: &str,
    sql: &str,
    params: Vec<Value>,
) -> Result<Value, String> {
    manager
        .call(
            ResourceCall {
                method: method.into(),
                args: json!({ "sql": sql, "params": params }),
            },
            CancellationToken::new(),
        )
        .await
        .map_err(|e| e.to_string())
}

/// Prepares the queue table on the backing database. Idempotent.
pub async fn ensure_schema(manager: &dyn ResourceManager) -> Result<(), String> {
    for statement in SCHEMA.split(';').map(str::trim).filter(|s| !s.is_empty()) {
        sql(manager, "execute", statement, vec![]).await?;
    }
    Ok(())
}

#[derive(Deserialize)]
struct Claimed {
    id: i64,
    payload: Value,
    attempts: i32,
}

/// Starts consumer loops for every queue workload of the revision. Returns
/// the token that stops them; the caller cancels it at drain. Each loop is
/// persistent infrastructure; each message is a fresh finite world.
pub fn start(
    runtime: Weak<Runtime>,
    revision: Arc<Revision>,
    stats: Arc<QueueStats>,
) -> CancellationToken {
    let stop = CancellationToken::new();
    for workload in revision.definition.workloads() {
        let Trigger::Queue {
            topic,
            concurrency,
            database,
            retry,
        } = &workload.trigger
        else {
            continue;
        };
        let Some(manager) = database_for(&revision, database.as_deref()) else {
            tracing::error!(queue = %workload.name, "no postgres resource backs this queue; consumer not started");
            continue;
        };
        let validator = workload
            .contracts
            .message
            .as_ref()
            .and_then(|schema| jsonschema::validator_for(schema).ok())
            .map(Arc::new);
        for worker in 0..(*concurrency).max(1) {
            let stop = stop.clone();
            let runtime = runtime.clone();
            let revision = Arc::clone(&revision);
            let manager = Arc::clone(&manager);
            let stats = Arc::clone(&stats);
            let validator = validator.clone();
            let retry = retry.clone();
            let topic = topic.clone();
            let id = workload.id.clone();
            let name = workload.name.clone();
            let deadline = workload.timeout_ms;
            tokio::spawn(async move {
                let locked_by = format!("{}:{}:{worker}", revision.id, name);
                if let Err(e) = ensure_schema(manager.as_ref()).await {
                    tracing::error!(queue = %name, error = %e, "queue schema unavailable; consumer stopping");
                    return;
                }
                loop {
                    if stop.is_cancelled() {
                        return;
                    }
                    let claimed = sql(
                        manager.as_ref(),
                        "one",
                        "UPDATE usai_queue SET state = 'processing', locked_at = now(), locked_by = $1, attempts = attempts + 1
                         WHERE id = (SELECT id FROM usai_queue WHERE topic = $2 AND state = 'ready' AND available_at <= now()
                                     ORDER BY id FOR UPDATE SKIP LOCKED LIMIT 1)
                         RETURNING id, payload, attempts",
                        vec![json!(locked_by), json!(topic)],
                    )
                    .await;
                    let claimed = match claimed {
                        Ok(Value::Null) => {
                            tokio::select! {
                                _ = tokio::time::sleep(POLL_INTERVAL) => continue,
                                _ = stop.cancelled() => return,
                            }
                        }
                        Ok(row) => match serde_json::from_value::<Claimed>(row) {
                            Ok(c) => c,
                            Err(e) => {
                                tracing::error!(queue = %name, error = %e, "undecodable claim");
                                continue;
                            }
                        },
                        Err(e) => {
                            tracing::warn!(queue = %name, error = %e, "claim failed; backing off");
                            tokio::select! {
                                _ = tokio::time::sleep(POLL_INTERVAL * 4) => continue,
                                _ = stop.cancelled() => return,
                            }
                        }
                    };
                    stats.claimed.fetch_add(1, Ordering::SeqCst);

                    // Boundary validation before any world exists (C6).
                    if let Some(validator) = &validator {
                        let issues: Vec<String> = validator
                            .iter_errors(&claimed.payload)
                            .map(|e| e.to_string())
                            .collect();
                        if !issues.is_empty() {
                            stats.invalid.fetch_add(1, Ordering::SeqCst);
                            let _ = sql(manager.as_ref(), "execute", "UPDATE usai_queue SET state = 'dead', last_error = $2 WHERE id = $1", vec![json!(claimed.id), json!(format!("message contract: {}", issues.join("; ")))]).await;
                            continue;
                        }
                    }

                    let Some(runtime) = runtime.upgrade() else {
                        return;
                    };
                    let outcome = run_message(
                        &runtime,
                        &revision,
                        &id,
                        &claimed,
                        deadline,
                        stop.child_token(),
                    )
                    .await;
                    match outcome {
                        Ok(()) => {
                            stats.done.fetch_add(1, Ordering::SeqCst);
                            let _ = sql(manager.as_ref(), "execute", "UPDATE usai_queue SET state = 'done', locked_at = NULL, locked_by = NULL WHERE id = $1", vec![json!(claimed.id)]).await;
                        }
                        Err(error) => {
                            let attempt = claimed.attempts.max(1) as u32;
                            if attempt < retry.max_attempts {
                                stats.retried.fetch_add(1, Ordering::SeqCst);
                                let delay = retry.delay_ms(attempt);
                                tracing::warn!(queue = %name, message = claimed.id, attempt, delay_ms = delay, error = %error, "message failed; retrying");
                                let _ = sql(
                                    manager.as_ref(),
                                    "execute",
                                    "UPDATE usai_queue SET state = 'ready', available_at = now() + ($2::bigint * interval '1 millisecond'), locked_at = NULL, locked_by = NULL, last_error = $3 WHERE id = $1",
                                    vec![json!(claimed.id), json!(delay), json!(error)],
                                )
                                .await;
                            } else {
                                stats.dead.fetch_add(1, Ordering::SeqCst);
                                tracing::error!(queue = %name, message = claimed.id, attempt, error = %error, "message dead-lettered");
                                let _ = sql(manager.as_ref(), "execute", "UPDATE usai_queue SET state = 'dead', locked_at = NULL, locked_by = NULL, last_error = $2 WHERE id = $1", vec![json!(claimed.id), json!(error)]).await;
                            }
                        }
                    }
                }
            });
        }
    }
    stop
}

async fn run_message(
    runtime: &Runtime,
    revision: &Arc<Revision>,
    workload_id: &str,
    claimed: &Claimed,
    _deadline: Option<u64>,
    cancel: CancellationToken,
) -> Result<(), String> {
    let admission = runtime
        .admit_in_flight(revision, workload_id)
        .map_err(|e| e.to_string())?;
    let input = super::input(
        revision,
        "queue",
        json!({ "message": claimed.payload, "id": claimed.id.to_string(), "attempt": claimed.attempts }),
    );
    let result = runtime
        .execute(admission, input, cancel)
        .await
        .map_err(|e| e.to_string())?;
    match (&result.termination, &result.outcome) {
        (Termination::Completed, Some(Ok(_))) => Ok(()),
        (Termination::Completed, Some(Err(e))) => Err(format!("{}: {}", e.name, e.message)),
        (Termination::Completed, None) => Err("no outcome".into()),
        (Termination::DeadlineExceeded, _) => Err("deadline exceeded".into()),
        (Termination::Cancelled { reason }, _) => Err(format!("cancelled: {reason}")),
        (Termination::Faulted { detail }, _) => Err(format!("faulted: {detail}")),
    }
}

/// `queue.publish`: inserts a message. The world owns the insert as an
/// external operation; delivery afterwards is the queue's.
pub struct PublishHandler;

#[derive(Deserialize)]
struct PublishRequest {
    topic: String,
    #[serde(default)]
    message: Value,
    #[serde(default)]
    database: Option<String>,
    #[serde(default)]
    delay_ms: Option<u64>,
}

impl OpHandler for PublishHandler {
    fn start(&self, ctx: OpContext, payload: String) -> Result<OpFuture, OpOutcome> {
        let request: PublishRequest = serde_json::from_str(&payload)
            .map_err(|e| OpOutcome::err("invalid_publish", 500, e.to_string()))?;
        let revision = ctx
            .revision
            .clone()
            .ok_or_else(|| OpOutcome::err("no_revision", 500, "world has no revision"))?;
        let manager = database_for(&revision, request.database.as_deref()).ok_or_else(|| {
            OpOutcome::err(
                "no_queue_database",
                500,
                "no postgres resource backs the queue",
            )
        })?;
        Ok(Box::pin(async move {
            if let Err(e) = ensure_schema(manager.as_ref()).await {
                return OpOutcome::err("queue_unavailable", 503, e);
            }
            let result = manager
                .call(
                    ResourceCall {
                        method: "one".into(),
                        args: json!({
                            "sql": "INSERT INTO usai_queue (topic, payload, available_at) VALUES ($1, $2::jsonb, now() + ($3::bigint * interval '1 millisecond')) RETURNING id",
                            "params": [request.topic, request.message, request.delay_ms.unwrap_or(0)],
                        }),
                    },
                    ctx.cancel.clone(),
                )
                .await;
            match result {
                Ok(row) => OpOutcome::ok(
                    &json!({ "id": row.get("id").cloned().unwrap_or(Value::Null).to_string() }),
                ),
                Err(e) => OpOutcome::from_resource_error(&e),
            }
        }))
    }
}

/// Counts of messages by state for `inspect`/status.
pub async fn depth(manager: &dyn ResourceManager, topic: &str) -> Result<Value, String> {
    sql(manager, "one", "SELECT count(*) FILTER (WHERE state = 'ready')::int AS ready, count(*) FILTER (WHERE state = 'processing')::int AS processing, count(*) FILTER (WHERE state = 'dead')::int AS dead, count(*) FILTER (WHERE state = 'done')::int AS done FROM usai_queue WHERE topic = $1", vec![json!(topic)]).await
}
