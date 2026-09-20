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
/// How often one sweeper per topic looks for messages a consumer claimed
/// and never finished.
const SWEEP_INTERVAL: Duration = Duration::from_secs(5);
/// A live consumer holds a row for at most the message deadline (the world
/// is cancelled at it and the row written back); a row still `processing`
/// this long after the deadline belongs to a process that is gone.
const LOST_MARGIN_MS: u64 = 10_000;

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
CREATE INDEX IF NOT EXISTS usai_queue_ready ON usai_queue (topic, available_at) WHERE state = 'ready';
CREATE INDEX IF NOT EXISTS usai_queue_claim ON usai_queue (topic, id) WHERE state = 'ready';
CREATE INDEX IF NOT EXISTS usai_queue_processing ON usai_queue (topic, locked_at) WHERE state = 'processing';";
// `usai_queue_claim` is what the claim walks: `ORDER BY id LIMIT 1` over the
// ready rows of a topic in id order, so a claim is three buffer reads
// whatever the backlog. Without it the planner sorted every ready row per
// claim (the queue campaign: 20 000 ready messages → 13 ms per claim, the
// consumers' whole budget). `usai_queue_processing` is the sweeper's.

#[derive(Default)]
pub struct QueueStats {
    pub claimed: AtomicU64,
    pub done: AtomicU64,
    pub retried: AtomicU64,
    pub dead: AtomicU64,
    pub invalid: AtomicU64,
    /// Messages a vanished consumer had claimed, put back for another
    /// attempt (or dead-lettered when it was the last one).
    pub reclaimed: AtomicU64,
}

pub(crate) fn database_for(
    revision: &Revision,
    name: Option<&str>,
) -> Option<Arc<dyn ResourceManager>> {
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

pub(crate) async fn sql(
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

/// A bookkeeping statement (claim, done, retry, dead): committed without
/// waiting for the WAL flush. Losing one to a crash redelivers a message,
/// which at-least-once already allows; the message's own effects (the
/// handler's writes) keep their synchronous commits.
async fn mark(
    manager: &dyn ResourceManager,
    method: &str,
    sql: &str,
    params: Vec<Value>,
) -> Result<Value, String> {
    manager
        .call(
            ResourceCall {
                method: method.into(),
                args: json!({ "sql": sql, "params": params, "async_commit": true }),
            },
            CancellationToken::new(),
        )
        .await
        .map_err(|e| e.to_string())
}

/// Databases whose queue table this process has already prepared, by the
/// resource's fingerprint: DDL costs a catalog lock and a commit, and it
/// used to run on **every publish** (the queue campaign measured 300
/// publishes/s on a database that takes 3 000 inserts/s — C13). Once per
/// database per process is the contract; a database recreated underneath a
/// running process is a `db migrate` matter, not a publish-time one.
static PREPARED: std::sync::Mutex<std::collections::BTreeSet<String>> =
    std::sync::Mutex::new(std::collections::BTreeSet::new());

/// `ensure_schema`, once per database per process.
pub async fn ensure_schema_once(manager: &dyn ResourceManager) -> Result<(), String> {
    let key = manager.identity().fingerprint.clone();
    if PREPARED.lock().expect("prepared poisoned").contains(&key) {
        return Ok(());
    }
    ensure_schema(manager).await?;
    PREPARED.lock().expect("prepared poisoned").insert(key);
    Ok(())
}

/// Prepares the queue table on the backing database. Idempotent, and safe
/// to run from several workers at once: `CREATE TABLE IF NOT EXISTS` is not
/// race-free in PostgreSQL (two sessions can both pass the existence check
/// and the loser gets 42P07 or a 23505 on `pg_type`), so a loser simply
/// runs the statements again, by which time the winner's objects exist.
pub async fn ensure_schema(manager: &dyn ResourceManager) -> Result<(), String> {
    let mut attempts = 0;
    loop {
        attempts += 1;
        let mut result = Ok(());
        for statement in SCHEMA.split(';').map(str::trim).filter(|s| !s.is_empty()) {
            result = sql(manager, "execute", statement, vec![]).await.map(|_| ());
            if result.is_err() {
                break;
            }
        }
        match result {
            Ok(()) => return Ok(()),
            Err(e) if attempts < 3 && (e.contains("sql_42p07") || e.contains("sql_23505")) => {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
            Err(e) => return Err(e),
        }
    }
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
        // One sweeper per topic: a consumer killed mid-message (SIGKILL, an
        // OOM kill, a host that died) leaves its rows `processing` with no
        // one to write them back. Measured on the two-replica campaign;
        // without this they stayed there forever. The row's `attempts` was
        // incremented at the claim, so the retry policy the application
        // declared is honoured: another attempt if any remain, dead
        // otherwise (ADR-0014 — the retry is the declaration's, not ours).
        {
            let stop = stop.clone();
            let runtime = runtime.clone();
            let manager = Arc::clone(&manager);
            let stats = Arc::clone(&stats);
            let topic = topic.clone();
            let name = workload.name.clone();
            let timeout_ms = workload.timeout_ms;
            let max_attempts = retry.max_attempts.max(1);
            tokio::spawn(async move {
                loop {
                    tokio::select! {
                        _ = tokio::time::sleep(SWEEP_INTERVAL) => {}
                        _ = stop.cancelled() => return,
                    }
                    let Some(runtime) = runtime.upgrade() else {
                        return;
                    };
                    let lost_after_ms = timeout_ms
                        .unwrap_or(runtime.config().default_timeout.as_millis() as u64)
                        + LOST_MARGIN_MS;
                    drop(runtime);
                    let reason = "consumer lost: claimed by ' || locked_by || ' at ' || to_char(locked_at at time zone 'UTC', 'YYYY-MM-DD HH24:MI:SS') || ' UTC, never completed";
                    let retried = sql(
                        manager.as_ref(),
                        "execute",
                        &format!("UPDATE usai_queue SET state = 'ready', available_at = now(), locked_at = NULL, locked_by = NULL, last_error = '{reason}' WHERE topic = $1 AND state = 'processing' AND locked_at < now() - ($2::bigint * interval '1 millisecond') AND attempts < $3"),
                        vec![json!(topic), json!(lost_after_ms), json!(max_attempts)],
                    )
                    .await;
                    let dead = sql(
                        manager.as_ref(),
                        "execute",
                        &format!("UPDATE usai_queue SET state = 'dead', locked_at = NULL, locked_by = NULL, last_error = '{reason}' WHERE topic = $1 AND state = 'processing' AND locked_at < now() - ($2::bigint * interval '1 millisecond') AND attempts >= $3"),
                        vec![json!(topic), json!(lost_after_ms), json!(max_attempts)],
                    )
                    .await;
                    let count = |r: &Result<Value, String>| {
                        r.as_ref().ok().and_then(Value::as_u64).unwrap_or(0)
                    };
                    let (retried, dead) = (count(&retried), count(&dead));
                    if retried + dead > 0 {
                        stats.reclaimed.fetch_add(retried + dead, Ordering::SeqCst);
                        stats.dead.fetch_add(dead, Ordering::SeqCst);
                        tracing::warn!(queue = %name, retried, dead, lost_after_ms, "messages a lost consumer had claimed were reclaimed");
                    }
                }
            });
        }
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
                // Empty claims in a row: the first may be a lost race with a
                // sibling worker on the head of the queue (the row it picked
                // was taken between the read and the lock), so it retries at
                // once; only a run of them means the topic is empty and the
                // poll interval applies.
                let mut empty_claims: u32 = 0;
                if let Err(e) = ensure_schema_once(manager.as_ref()).await {
                    tracing::error!(queue = %name, error = %e, "queue schema unavailable; consumer stopping");
                    return;
                }
                loop {
                    if stop.is_cancelled() {
                        return;
                    }
                    let claimed = mark(
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
                            empty_claims += 1;
                            let wait = match empty_claims {
                                1 => Duration::ZERO,
                                2 => Duration::from_millis(10),
                                _ => POLL_INTERVAL,
                            };
                            tokio::select! {
                                _ = tokio::time::sleep(wait) => continue,
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
                    empty_claims = 0;
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
                            let _ = mark(manager.as_ref(), "execute", "UPDATE usai_queue SET state = 'done', locked_at = NULL, locked_by = NULL WHERE id = $1", vec![json!(claimed.id)]).await;
                        }
                        Err(error) => {
                            let attempt = claimed.attempts.max(1) as u32;
                            if attempt < retry.max_attempts {
                                stats.retried.fetch_add(1, Ordering::SeqCst);
                                let delay = retry.delay_ms(attempt);
                                tracing::warn!(queue = %name, id = claimed.id, attempt, delay_ms = delay, error = %error, "message failed; retrying");
                                let _ = mark(
                                    manager.as_ref(),
                                    "execute",
                                    "UPDATE usai_queue SET state = 'ready', available_at = now() + ($2::bigint * interval '1 millisecond'), locked_at = NULL, locked_by = NULL, last_error = $3 WHERE id = $1",
                                    vec![json!(claimed.id), json!(delay), json!(error)],
                                )
                                .await;
                            } else {
                                stats.dead.fetch_add(1, Ordering::SeqCst);
                                tracing::error!(queue = %name, id = claimed.id, attempt, error = %error, "message dead-lettered");
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
        .execute_with_stop(admission, input, cancel, Some(revision.connections_stop()))
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
            if let Err(e) = ensure_schema_once(manager.as_ref()).await {
                return OpOutcome::err("queue_unavailable", 503, e);
            }
            let insert = || {
                manager.call(
                    ResourceCall {
                        method: "one".into(),
                        args: json!({
                            "sql": "INSERT INTO usai_queue (topic, payload, available_at) VALUES ($1, $2::jsonb, now() + ($3::bigint * interval '1 millisecond')) RETURNING id",
                            "params": [request.topic, request.message, request.delay_ms.unwrap_or(0)],
                        }),
                    },
                    ctx.cancel.clone(),
                )
            };
            let mut result = insert().await;
            // The table vanished under a prepared process (a schema reset in
            // development): prepare again, once, and retry.
            if let Err(e) = &result
                && e.to_string().contains("usai_queue")
                && e.to_string().contains("does not exist")
            {
                PREPARED
                    .lock()
                    .expect("prepared poisoned")
                    .remove(&manager.identity().fingerprint);
                if ensure_schema_once(manager.as_ref()).await.is_ok() {
                    result = insert().await;
                }
            }
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
