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

/// Operator verbs over `usai_queue`. They live beside the schema they
/// maintain, because both halves of "what the table is" have to agree.
///
/// The table is the application's data, not the runtime's: nothing here runs
/// on its own, and nothing prunes without being asked.
pub mod ops {
    use super::SCHEMA;
    use crate::resource::{ResourceCall, ResourceError, ResourceManager};
    use serde_json::{Value, json};
    use tokio_util::sync::CancellationToken;

    async fn sql(
        manager: &dyn ResourceManager,
        method: &str,
        statement: &str,
        params: Vec<Value>,
    ) -> Result<Value, ResourceError> {
        manager
            .call(
                ResourceCall {
                    method: method.to_owned(),
                    args: json!({ "sql": statement, "params": params }),
                },
                CancellationToken::new(),
            )
            .await
    }

    /// What is in the table, per topic and state, with the ages an operator
    /// actually pages on: the oldest thing still waiting, and the oldest
    /// thing still claimed.
    pub async fn stats(manager: &dyn ResourceManager) -> Result<Value, ResourceError> {
        sql(
            manager,
            "query",
            "SELECT topic, state, count(*)::bigint AS rows,
                    max(extract(epoch FROM now() - available_at))::bigint AS oldest_wait_seconds,
                    max(extract(epoch FROM now() - locked_at))::bigint AS oldest_claim_seconds
             FROM usai_queue GROUP BY topic, state ORDER BY topic, state",
            vec![],
        )
        .await
    }

    /// Deletes finished rows. `state` is `done`, `dead` or both; `older_than`
    /// is seconds, measured from the message's `created_at` — the one column
    /// that does not move (`available_at` is pushed forward by every retry,
    /// so a message that was retried would look younger than it is). Returns
    /// how many rows went.
    ///
    /// The runtime never does this by itself: a `dead` row is a message an
    /// application failed to process, and only the application's owner knows
    /// whether it is still evidence.
    pub async fn prune(
        manager: &dyn ResourceManager,
        states: &[&str],
        older_than_seconds: i64,
        topic: Option<&str>,
    ) -> Result<i64, ResourceError> {
        let list: Vec<Value> = states.iter().map(|s| json!(s)).collect();
        let affected = sql(
            manager,
            "execute",
            "DELETE FROM usai_queue
             WHERE state = ANY($1::text[])
               AND created_at < now() - ($2::bigint * interval '1 second')
               AND ($3::text IS NULL OR topic = $3)",
            vec![json!(list), json!(older_than_seconds), json!(topic)],
        )
        .await?;
        Ok(affected.as_i64().unwrap_or(0))
    }

    /// Exactly what `prune` would delete, without deleting it. The same
    /// predicate, because a dry run whose number is not the delete's number
    /// is worse than no dry run: `--dry-run` exists so an operator can read
    /// the count before typing `--yes`, and it used to count by state alone
    /// and ignore the age — "this would delete 4.1 M rows" when the answer
    /// was 900.
    pub async fn prune_count(
        manager: &dyn ResourceManager,
        states: &[&str],
        older_than_seconds: i64,
        topic: Option<&str>,
    ) -> Result<i64, ResourceError> {
        let list: Vec<Value> = states.iter().map(|s| json!(s)).collect();
        let row = sql(
            manager,
            "one",
            "SELECT count(*)::bigint AS rows FROM usai_queue
             WHERE state = ANY($1::text[])
               AND created_at < now() - ($2::bigint * interval '1 second')
               AND ($3::text IS NULL OR topic = $3)",
            vec![json!(list), json!(older_than_seconds), json!(topic)],
        )
        .await?;
        Ok(row.get("rows").and_then(Value::as_i64).unwrap_or(0))
    }

    /// Creates the schema's indexes with `CREATE INDEX CONCURRENTLY`, so an
    /// upgrade does not build them under a write-blocking lock on a table
    /// that may hold millions of rows. Safe to run repeatedly and while the
    /// application serves; run it *before* deploying a version that adds an
    /// index. Returns the statements it ran.
    ///
    /// `CONCURRENTLY` cannot run inside a transaction, which is why this is
    /// a verb rather than a migration.
    pub async fn prepare_indexes(
        manager: &dyn ResourceManager,
    ) -> Result<Vec<String>, ResourceError> {
        // The table itself first: an index needs something to index.
        let table = SCHEMA
            .split(';')
            .map(str::trim)
            .find(|s| s.starts_with("CREATE TABLE"))
            .unwrap_or_default();
        if !table.is_empty() {
            sql(manager, "execute", table, vec![]).await?;
        }
        let mut ran = Vec::new();
        for statement in SCHEMA.split(';').map(str::trim) {
            let Some(rest) = statement.strip_prefix("CREATE INDEX IF NOT EXISTS ") else {
                continue;
            };
            let concurrent = format!("CREATE INDEX CONCURRENTLY IF NOT EXISTS {rest}");
            sql(manager, "execute", &concurrent, vec![]).await?;
            ran.push(concurrent);
        }
        Ok(ran)
    }
}

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
  created_at timestamptz NOT NULL DEFAULT now(),
  request_id text
);
ALTER TABLE usai_queue ADD COLUMN IF NOT EXISTS request_id text;
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
    /// The same six counts per topic. An alert on dead letters that cannot
    /// name the queue sends whoever it woke to look through every topic the
    /// application has, and HTTP has carried its per-workload dimension
    /// since D2 — this was an inconsistency, not a design stance. Bounded by
    /// the topics the definition declares, never by message data.
    pub by_topic: std::sync::RwLock<std::collections::BTreeMap<String, QueueTopicCounters>>,
}

/// One topic's counts. Same six as above.
#[derive(Debug, Default)]
pub struct QueueTopicCounters {
    pub claimed: AtomicU64,
    pub done: AtomicU64,
    pub retried: AtomicU64,
    pub dead: AtomicU64,
    pub invalid: AtomicU64,
    pub reclaimed: AtomicU64,
}

impl QueueStats {
    /// Adds one to a counter, for the revision and for the topic.
    pub fn count(&self, topic: &str, pick: fn(&QueueTopicCounters) -> &AtomicU64) {
        if let Some(counters) = self
            .by_topic
            .read()
            .expect("queue stats poisoned")
            .get(topic)
        {
            pick(counters).fetch_add(1, Ordering::SeqCst);
            return;
        }
        let mut map = self.by_topic.write().expect("queue stats poisoned");
        pick(map.entry(topic.to_owned()).or_default()).fetch_add(1, Ordering::SeqCst);
    }
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

/// The mark that records a message's outcome, which must not be lost: the
/// row stays `processing` when it is, and the sweeper then redelivers or
/// dead-letters work that was already done (found running a 0.0.5 and a
/// 0.0.6 consumer against one table while a third process saturated the
/// pool). The statement is idempotent — it names the row by id — so a
/// failure is retried once, synchronously this time, and a second failure
/// is logged with what the operator will see because of it.
/// `mark_outcome` for the integration tests.
pub async fn mark_outcome_for_test(
    manager: &dyn ResourceManager,
    queue: &str,
    id: i64,
    outcome: &str,
    sql_text: &str,
    params: Vec<Value>,
) {
    mark_outcome(manager, queue, id, outcome, sql_text, params).await
}

async fn mark_outcome(
    manager: &dyn ResourceManager,
    queue: &str,
    id: i64,
    outcome: &str,
    sql_text: &str,
    params: Vec<Value>,
) {
    let Err(first) = mark(manager, "execute", sql_text, params.clone()).await else {
        return;
    };
    tokio::time::sleep(Duration::from_millis(50)).await;
    // No `async_commit` on the retry: the whole point is to know it landed.
    if let Err(second) = sql(manager, "execute", sql_text, params).await {
        tracing::warn!(
            queue = %queue,
            id,
            outcome,
            error = %second,
            first_error = %first,
            "could not record the message's outcome; the row stays `processing` until the lost-consumer sweep redelivers it (or dead-letters it when its attempts are spent) — the handler already ran"
        );
    }
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
/// race-free in PostgreSQL. Two sessions can both pass the existence check,
/// and the loser fails in one of three ways depending on which catalog it
/// lost on — **42P07** (the relation), **42710** (the table's implicit row
/// type, `duplicate_object`) or **23505** (a unique violation on `pg_type`).
/// A loser simply runs the statements again, by which time the winner's
/// objects exist. Missing 42710 from that list is how a CI run with several
/// consumers starting together failed on
/// `type "usai_queue" already exists` — which is what the first publish
/// from a fresh multi-replica deployment does.
pub async fn ensure_schema(manager: &dyn ResourceManager) -> Result<(), String> {
    let mut attempts = 0;
    loop {
        attempts += 1;
        let mut result = Ok(());
        for statement in SCHEMA.split(';').map(str::trim).filter(|s| !s.is_empty()) {
            // Every statement is `IF NOT EXISTS`, so on a prepared database
            // this is a catalog lookup. On a database whose table predates
            // this version it is real work — an index over an existing
            // `usai_queue` builds under a lock that blocks publishes and
            // claims — so a slow one says what it was doing and for how
            // long, instead of looking like a stall with no cause.
            let started = std::time::Instant::now();
            result = sql(manager, "execute", statement, vec![]).await.map(|_| ());
            let elapsed = started.elapsed();
            if elapsed > std::time::Duration::from_millis(500) {
                let first_line = statement.lines().next().unwrap_or(statement);
                tracing::warn!(
                    ms = elapsed.as_millis() as u64,
                    statement = first_line,
                    "preparing the queue schema took a while: this statement was building on an existing usai_queue and held a lock on it. Prune the table (done/dead rows are yours to delete) or create the index with CREATE INDEX CONCURRENTLY before the upgrade"
                );
            }
            if result.is_err() {
                break;
            }
        }
        match result {
            Ok(()) => return Ok(()),
            Err(e)
                if attempts < 5
                    && ["sql_42p07", "sql_42710", "sql_23505"]
                        .iter()
                        .any(|code| e.contains(code)) =>
            {
                tokio::time::sleep(std::time::Duration::from_millis(50 * attempts)).await;
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
    /// The request id of the world that published the message, so the
    /// consumer's world runs under it (`ctx.requestId`, its log lines).
    #[serde(default)]
    request_id: Option<String>,
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
            .and_then(|schema| {
                // Formats are asserted here too (see http/router.rs).
                jsonschema::options()
                    .should_validate_formats(true)
                    .build(schema)
                    .ok()
            })
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
                        for _ in 0..(retried + dead) {
                            stats.count(&name, |c| &c.reclaimed);
                        }
                        for _ in 0..dead {
                            stats.count(&name, |c| &c.dead);
                        }
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
                         RETURNING id, payload, attempts, request_id",
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
                    stats.count(&name, |c| &c.claimed);

                    // Boundary validation before any world exists (C6).
                    if let Some(validator) = &validator {
                        let issues: Vec<String> = validator
                            .iter_errors(&claimed.payload)
                            .map(|e| e.to_string())
                            .collect();
                        if !issues.is_empty() {
                            stats.invalid.fetch_add(1, Ordering::SeqCst);
                            stats.count(&name, |c| &c.invalid);
                            let detail = issues.join("; ");
                            // A message rejected by its contract is dead on
                            // arrival, and used to be dead *silently*: a row
                            // in a table and a counter with no topic on it.
                            // That is the shape of the commonest queue
                            // incident there is — a producer deployed with a
                            // new payload before the consumer's schema knew
                            // about it — and nothing reached the log, so
                            // nothing reached a log pipeline. A handler that
                            // fails logs an ERROR; so does this.
                            tracing::error!(
                                queue = %name,
                                id = claimed.id,
                                error = %detail,
                                "message dead-lettered: it does not match the topic's contract, so no world ran for it"
                            );
                            let _ = sql(manager.as_ref(), "execute", "UPDATE usai_queue SET state = 'dead', last_error = $2 WHERE id = $1", vec![json!(claimed.id), json!(format!("message contract: {detail}"))]).await;
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
                            stats.count(&name, |c| &c.done);
                            mark_outcome(
                                manager.as_ref(),
                                &name,
                                claimed.id,
                                "done",
                                "UPDATE usai_queue SET state = 'done', locked_at = NULL, locked_by = NULL WHERE id = $1",
                                vec![json!(claimed.id)],
                            )
                            .await;
                        }
                        Err(error) => {
                            let attempt = claimed.attempts.max(1) as u32;
                            if attempt < retry.max_attempts {
                                stats.retried.fetch_add(1, Ordering::SeqCst);
                                stats.count(&name, |c| &c.retried);
                                let delay = retry.delay_ms(attempt);
                                tracing::warn!(queue = %name, id = claimed.id, attempt, delay_ms = delay, error = %error, "message failed; retrying");
                                mark_outcome(
                                    manager.as_ref(),
                                    &name,
                                    claimed.id,
                                    "retry",
                                    "UPDATE usai_queue SET state = 'ready', available_at = now() + ($2::bigint * interval '1 millisecond'), locked_at = NULL, locked_by = NULL, last_error = $3 WHERE id = $1",
                                    vec![json!(claimed.id), json!(delay), json!(error)],
                                )
                                .await;
                            } else {
                                stats.dead.fetch_add(1, Ordering::SeqCst);
                                stats.count(&name, |c| &c.dead);
                                tracing::error!(queue = %name, id = claimed.id, attempt, error = %error, "message dead-lettered");
                                mark_outcome(
                                    manager.as_ref(),
                                    &name,
                                    claimed.id,
                                    "dead",
                                    "UPDATE usai_queue SET state = 'dead', locked_at = NULL, locked_by = NULL, last_error = $2 WHERE id = $1",
                                    vec![json!(claimed.id), json!(error)],
                                )
                                .await;
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
        json!({ "message": claimed.payload, "id": claimed.id.to_string(), "attempt": claimed.attempts, "requestId": claimed.request_id }),
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
                            "sql": "INSERT INTO usai_queue (topic, payload, available_at, request_id) VALUES ($1, $2::jsonb, now() + ($3::bigint * interval '1 millisecond'), $4) RETURNING id",
                            "params": [request.topic, request.message, request.delay_ms.unwrap_or(0), ctx.request_id.as_deref()],
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
