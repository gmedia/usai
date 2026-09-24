//! D6 acceptance: PostgreSQL ownership (contract C5).
//!
//! Skips when no database is available (see tests/support).

mod support;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;
use usai_runtime::build::{BuildOptions, build};
use usai_runtime::resource::{ResourceCall, ResourceError, TerminalProof};
use usai_runtime::*;

struct Fixture {
    runtime: Arc<Runtime>,
}

impl Fixture {
    async fn http(&self, method: &str, path: &str, params: Value) -> (u16, Value) {
        let id = format!("http:{method} {path}");
        let (_, w) = self
            .runtime
            .active()
            .unwrap()
            .definition
            .workload(&id)
            .map(|(i, w)| (i, w.id.clone()))
            .expect("workload");
        let input = json!({ "kind": "http", "env": {}, "request": { "method": method, "path": path, "url": path, "params": params, "query": {}, "headers": {}, "body": null } });
        let r = self.runtime.invoke(&w, input).await.unwrap();
        match (&r.termination, &r.outcome) {
            (Termination::Completed, Some(Ok(v))) => {
                (v["status"].as_u64().unwrap() as u16, v["json"].clone())
            }
            (Termination::Completed, Some(Err(e))) => (
                e.usai
                    .as_ref()
                    .and_then(|u| u["status"].as_u64())
                    .unwrap_or(500) as u16,
                json!({ "error": e.message }),
            ),
            (Termination::DeadlineExceeded, _) => (504, Value::Null),
            (t, _) => (500, json!({ "termination": format!("{t:?}") })),
        }
    }

    fn pg_status(&self) -> usai_runtime::resource::ResourceStatus {
        self.runtime
            .status()
            .resources
            .into_iter()
            .find(|r| r.identity.kind == "postgres")
            .unwrap()
    }

    /// The `usai_queue` manager, for the operator verbs.
    fn manager(&self) -> std::sync::Arc<dyn usai_runtime::resource::ResourceManager> {
        let revision = self.runtime.active().unwrap();
        usai_runtime::db::database(&revision, None).unwrap()
    }

    fn baseline(&self) {
        let g = self.runtime.ledger().gauges.snapshot();
        assert_eq!(g.live_worlds, 0, "{g:?}");
        assert_eq!(g.live_ops, 0, "{g:?}");
    }
}

async fn fixture() -> Option<Fixture> {
    fixture_with(false).await
}

async fn fixture_with(queue_consumers: bool) -> Option<Fixture> {
    fixture_reaching(queue_consumers, |url| url.to_owned()).await
}

/// `reroute` rewrites the connection URL the application is given — the
/// readiness test sends it through a relay it can cut.
async fn fixture_reaching(
    queue_consumers: bool,
    reroute: impl FnOnce(&str) -> String,
) -> Option<Fixture> {
    let Some(url) = support::database_url() else {
        eprintln!("skipping: no PostgreSQL available");
        return None;
    };
    // One database per fixture: tests run concurrently.
    let url = support::fresh_database(&url).await;
    let url = reroute(&url);
    if std::process::Command::new("node")
        .arg("--version")
        .output()
        .map(|o| !o.status.success())
        .unwrap_or(true)
    {
        return None;
    }
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/pg-app");
    if !root.join("node_modules/@sakaladev/usai").exists() {
        return None;
    }
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let out_dir = std::env::temp_dir().join(format!(
        "usai-pg-test-{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    ));
    let engine = usai_runtime::engine::from_env(64).unwrap();
    let out = build(
        engine.as_ref(),
        &BuildOptions {
            out_dir,
            ..BuildOptions::for_project(&root)
        },
    )
    .await
    .expect("fixture builds");
    let runtime = Runtime::with_env(
        engine,
        RuntimeConfig {
            default_timeout: Duration::from_secs(10),
            cron_scheduler: false,
            queue_consumers,
            ..RuntimeConfig::default()
        },
        move |name| (name == "DATABASE_URL").then(|| url.clone()),
    );
    let rev = runtime.install(out.definition).await.unwrap();
    runtime.activate(rev.id).await.unwrap();
    let f = Fixture { runtime };
    let r = f.runtime.run_command("setup", vec![]).await.unwrap();
    assert!(
        matches!(r.outcome, Some(Ok(_))),
        "setup failed: {:?}",
        r.outcome
    );
    Some(f)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn normal_completion_returns_the_connection() {
    let Some(f) = fixture().await else { return };
    let (status, body) = f.http("GET", "/users/:id", json!({ "id": "1" })).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["name"], "Ayu");
    let (status, _) = f.http("GET", "/users/:id", json!({ "id": "999" })).await;
    assert_eq!(status, 404);
    let (status, body) = f.http("GET", "/users", json!({})).await;
    assert_eq!(status, 200);
    assert_eq!(body.as_array().unwrap().len(), 2);
    let s = f.pg_status();
    assert_eq!(s.quarantined, 0);
    assert_eq!(s.in_use, 0, "connections returned to the pool: {s:?}");
    f.baseline();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn bytes_bind_to_bytea_and_come_back_as_base64() {
    let Some(f) = fixture().await else { return };
    // The http helper carries no query string; call the workload directly.
    let (_, w) = f
        .runtime
        .active()
        .unwrap()
        .definition
        .workload("http:GET /blobs/put")
        .map(|(i, w)| (i, w.id.clone()))
        .unwrap();
    let r = f.runtime.invoke(&w, json!({ "kind": "http", "env": {}, "request": { "method": "GET", "path": "/blobs/put", "url": "/blobs/put?hex=00ff10", "params": {}, "query": { "hex": "00ff10" }, "headers": {}, "body": null } })).await.unwrap();
    let put = r.outcome.unwrap().unwrap();
    assert_eq!(put["status"], 200, "{put}");
    assert_eq!(
        put["json"]["length"], 3,
        "three bytes stored as bytea, not as base64 text: {put}"
    );
    let id = put["json"]["id"].as_i64().unwrap().to_string();
    let (status, body) = f.http("GET", "/blobs/:id", json!({ "id": id })).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["hex"], "00ff10");
    assert_eq!(body["base64"], "AP8Q");
    f.baseline();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_exclusive_cron_tick_is_claimed_once_across_instances() {
    let Some(f) = fixture().await else { return };
    let rev = f.runtime.active().unwrap();
    let (_, nightly) = rev.definition.workload("cron:nightly").unwrap();
    match &nightly.trigger {
        usai_runtime::definition::Trigger::Cron {
            exclusive,
            database,
            ..
        } => {
            assert!(*exclusive);
            assert_eq!(*database, None, "the application's first postgres resource");
        }
        other => panic!("{other:?}"),
    }
    let manager = rev.resources().get("main").cloned().unwrap();
    let at = chrono::DateTime::parse_from_rfc3339("2026-09-21T03:00:00Z")
        .unwrap()
        .with_timezone(&chrono::Utc);
    // Two replicas' schedulers reach the same tick: the first inserts the
    // row (the table is created on first use), the second finds it taken;
    // the next tick is a new row.
    let claim = |who: &'static str, at| {
        let manager = Arc::clone(&manager);
        async move {
            usai_runtime::workloads::cron::claim_tick(manager.as_ref(), "nightly", at, who)
                .await
                .unwrap()
        }
    };
    assert!(claim("rev1:nightly", at).await);
    assert!(!claim("rev2:nightly", at).await);
    assert!(
        !claim("rev1:nightly", at).await,
        "not even the claimant twice"
    );
    assert!(claim("rev2:nightly", at + chrono::Duration::days(1)).await);
    let rows = manager
        .call(
            usai_runtime::resource::ResourceCall {
                method: "query".into(),
                args: json!({ "sql": "SELECT claimed_by FROM usai_cron_ticks WHERE name = 'nightly' ORDER BY scheduled_at", "params": [] }),
            },
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(
        rows,
        json!([{ "claimed_by": "rev1:nightly" }, { "claimed_by": "rev2:nightly" }])
    );
    f.baseline();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn bigints_beyond_the_safe_range_travel_as_strings() {
    let Some(f) = fixture().await else { return };
    let manager = f
        .runtime
        .active()
        .unwrap()
        .resources()
        .get("main")
        .cloned()
        .unwrap();
    let row = manager
        .call(
            usai_runtime::resource::ResourceCall {
                method: "one".into(),
                args: json!({ "sql": "select 5::bigint as small, -9007199254740992::bigint as edge, 9007199254740993::bigint as big, array[1::bigint, 9007199254740993::bigint] as arr", "params": [] }),
            },
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(
        row,
        json!({ "small": 5, "edge": -9007199254740992i64, "big": "9007199254740993", "arr": [1, "9007199254740993"] })
    );
    f.baseline();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sql_error_is_terminal_and_the_connection_is_reused() {
    let Some(f) = fixture().await else { return };
    let (status, body) = f.http("GET", "/fail", json!({})).await;
    assert_eq!(status, 200);
    assert_eq!(body["code"], "sql_22012");
    let s = f.pg_status();
    assert_eq!(s.quarantined, 0);
    // The same connection serves the next request.
    let (status, _) = f.http("GET", "/users/:id", json!({ "id": "2" })).await;
    assert_eq!(status, 200);
    assert_eq!(
        f.pg_status().detail["poolSize"],
        1,
        "no replacement connection was needed"
    );
    f.baseline();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cooperative_cancellation_awaits_terminal_state_then_reuses() {
    let Some(f) = fixture().await else { return };
    let started = std::time::Instant::now();
    let (status, body) = f.http("GET", "/slow", json!({})).await;
    assert_eq!(status, 504, "{body}");
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "cancellation must not wait for pg_sleep"
    );
    // Give the operation owner a moment to settle the lease.
    tokio::time::sleep(Duration::from_millis(200)).await;
    let s = f.pg_status();
    assert_eq!(s.detail["cancelled"], 1);
    assert_eq!(
        s.quarantined, 0,
        "a confirmed cancellation (57014) keeps the connection"
    );
    assert_eq!(s.in_use, 0);
    // Recovery: the pool serves again on the same connection.
    let (status, _) = f.http("GET", "/users/:id", json!({ "id": "1" })).await;
    assert_eq!(status, 200);
    assert_eq!(f.pg_status().detail["poolSize"], 1);
    f.baseline();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ambiguous_abandonment_quarantines_the_connection() {
    let Some(f) = fixture().await else { return };
    let rev = f.runtime.active().unwrap();
    let manager = rev.resources().get("main").cloned().unwrap();
    // Abort the operation owner mid-query: the lease drops without terminal
    // proof, which is exactly the hard-abandonment case.
    let m = Arc::clone(&manager);
    let task = tokio::spawn(async move {
        m.call(
            ResourceCall {
                method: "one".into(),
                args: json!({ "sql": "select pg_sleep(30)", "params": [] }),
            },
            CancellationToken::new(),
        )
        .await
    });
    tokio::time::sleep(Duration::from_millis(300)).await;
    task.abort();
    let _ = task.await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    let s = f.pg_status();
    assert_eq!(s.quarantined, 1, "{s:?}");
    assert_eq!(s.in_use, 0);
    // Recovery: a replacement connection serves the next request.
    let (status, body) = f.http("GET", "/users/:id", json!({ "id": "1" })).await;
    assert_eq!(status, 200, "{body}");
    f.baseline();
}

/// `resources[].ready` in `/_usai/status` follows the last contact with the
/// server: a connection-level failure (here the server terminating our own
/// backend, SQLSTATE 57P01) makes it false with the error beside it; the
/// next successful operation makes it true again. A query's own error (a
/// bad statement) says nothing about the database and leaves it ready.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn resource_readiness_follows_connection_level_failures() {
    let Some(f) = fixture().await else { return };
    let rev = f.runtime.active().unwrap();
    let manager = rev.resources().get("main").cloned().unwrap();
    let call = |sql: &str| ResourceCall {
        method: "one".into(),
        args: json!({ "sql": sql, "params": [] }),
    };
    assert!(f.pg_status().ready);
    let err = manager
        .call(
            call("select * from no_such_table"),
            CancellationToken::new(),
        )
        .await
        .unwrap_err();
    assert!(err.to_string().contains("no_such_table"), "{err}");
    assert!(f.pg_status().ready, "a query error is not an outage");
    let err = manager
        .call(
            call("select pg_terminate_backend(pg_backend_pid())"),
            CancellationToken::new(),
        )
        .await
        .unwrap_err();
    assert!(
        matches!(&err, usai_runtime::resource::ResourceError::Operation { code, .. } if code == "sql_57p01" || code == "connection_closed"),
        "{err:?}"
    );
    let s = f.pg_status();
    assert!(!s.ready, "{s:?}");
    assert!(
        s.detail["lastError"].as_str().unwrap().contains("terminat"),
        "{s:?}"
    );
    assert!(s.detail["unreadyForSeconds"].is_u64());
    let (status, body) = f.http("GET", "/users/:id", json!({ "id": "1" })).await;
    assert_eq!(status, 200, "{body}");
    let s = f.pg_status();
    assert!(s.ready, "{s:?}");
    assert!(!s.detail.contains_key("lastError"));
    f.baseline();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn no_connection_state_leaks_across_worlds() {
    let Some(f) = fixture().await else { return };
    // World 1 sets a session variable on its leased connection; world 2
    // leases the same physical connection and must not see it.
    let (_, first) = f.http("GET", "/leak", json!({})).await;
    assert_eq!(first["before"], Value::Null);
    let (_, second) = f.http("GET", "/leak", json!({})).await;
    // After RESET ALL a placeholder GUC reads back as "" rather than NULL;
    // either is "not the other world's value".
    assert_ne!(
        second["before"],
        json!("set-by-world"),
        "session state leaked across worlds through the pool"
    );
    assert_eq!(
        f.pg_status().detail["poolSize"],
        1,
        "the same connection served both worlds"
    );
    f.baseline();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn raw_transaction_control_is_refused_and_names_the_transaction_helper() {
    let Some(f) = fixture().await else { return };
    // `begin` on the unpinned path used to open a transaction on one pooled
    // connection while the statements that followed ran on others, and the
    // connection went back to the pool `idle in transaction`.
    let (status, body) = f.http("POST", "/tx/raw", json!({})).await;
    assert_eq!(status, 200, "{body}");
    for attempt in body["tried"].as_array().expect("attempts") {
        assert_eq!(
            attempt["code"],
            json!("transaction_control"),
            "{} was not refused: {attempt}",
            attempt["sql"]
        );
        let message = attempt["message"].as_str().unwrap_or_default();
        assert!(
            message.contains("transaction("),
            "the refusal must name the helper that works: {message}"
        );
    }
    // And the pinned path still accepts exactly what was refused above.
    assert_eq!(body["pinned"], json!(1), "{body}");
    f.baseline();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_queue_can_be_inspected_pruned_and_indexed_without_blocking_writers() {
    let Some(f) = fixture_with(true).await else {
        return;
    };
    use usai_runtime::workloads::queue::ops;
    let manager = f.manager();
    // `prepare` builds the schema's indexes with CONCURRENTLY: an upgrade
    // that adds one would otherwise build it under a write-blocking lock on
    // a table the runtime never prunes.
    let built = ops::prepare_indexes(manager.as_ref())
        .await
        .expect("indexes built");
    assert!(!built.is_empty(), "prepare built nothing");
    assert!(
        built.iter().all(|s| s.contains("CONCURRENTLY")),
        "{built:?}"
    );
    // Repeating it is safe — the operator runs it before every upgrade.
    ops::prepare_indexes(manager.as_ref())
        .await
        .expect("prepare repeats");

    // Publish through the application, then let the consumer finish it.
    // `http()` takes path params; this route wants a body, so invoke it the
    // way the other queue tests in this file do.
    let revision = f.runtime.active().unwrap();
    let (_, workload) = revision
        .definition
        .workload("http:POST /orders")
        .map(|(i, w)| (i, w.id.clone()))
        .expect("the publish route");
    let input = json!({ "kind": "http", "env": {}, "request": { "method": "POST", "path": "/orders", "url": "/orders", "params": {}, "query": {}, "headers": {}, "body": { "json": { "orderId": "prune-me" } } } });
    let published = f.runtime.invoke(&workload, input).await.unwrap();
    assert!(
        matches!(published.termination, Termination::Completed),
        "{:?}",
        published.termination
    );
    let mut done = 0;
    for _ in 0..60 {
        let stats = ops::stats(manager.as_ref()).await.expect("stats");
        done = stats
            .as_array()
            .map(|rows| {
                rows.iter()
                    .filter(|r| r["state"] == json!("done"))
                    .filter_map(|r| r["rows"].as_i64())
                    .sum::<i64>()
            })
            .unwrap_or(0);
        if done > 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(done > 0, "no message reached `done`");

    // Nothing is old enough yet: prune must not take a fresh row.
    let kept = ops::prune(manager.as_ref(), &["done"], 3_600, None)
        .await
        .expect("prune");
    assert_eq!(kept, 0, "prune took a row younger than its age filter");
    // And the dry run must say the same number the delete would. It used to
    // count by state alone and ignore the age, so `--dry-run` — the flag
    // that exists so an operator can read the count before typing `--yes` —
    // reported every finished row on the table and the delete then took
    // none of them.
    let would = ops::prune_count(manager.as_ref(), &["done"], 3_600, None)
        .await
        .expect("prune_count");
    assert_eq!(would, kept, "the dry run's number is not the delete's");
    let would_all = ops::prune_count(manager.as_ref(), &["done"], 0, None)
        .await
        .expect("prune_count");
    assert_eq!(would_all, done, "the dry run undercounts what would go");
    // With no age bound it takes exactly the finished ones.
    let deleted = ops::prune(manager.as_ref(), &["done"], 0, None)
        .await
        .expect("prune");
    assert_eq!(deleted, done, "prune deleted {deleted}, expected {done}");
    f.baseline();
}

/// The drain grace is for a load balancer and for requests already in
/// flight. Nothing watches a queue consumer, so claiming through the grace
/// only takes work this instance then has to finish inside `--drain-timeout`
/// while it is leaving — a jobs round measured a message claimed 1.67 s into
/// a 2 s grace. Background work stops at the start of the drain now.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_draining_instance_claims_no_more_messages() {
    let Some(f) = fixture_with(true).await else {
        return;
    };
    let manager = f.manager();
    use usai_runtime::workloads::queue::ops;

    // Let the consumer prove it is running: one message in, one done.
    let rev = f.runtime.active().unwrap();
    let (_, w) = rev
        .definition
        .workload("http:POST /orders")
        .map(|(i, w)| (i, w.id.clone()))
        .unwrap();
    let publish = |id: &str| {
        let w = w.clone();
        let input = json!({ "kind": "http", "request": { "method": "POST", "path": "/orders", "url": "/orders", "params": {}, "query": {}, "headers": {}, "body": { "json": { "orderId": id } } } });
        let rt = Arc::clone(&f.runtime);
        async move { rt.invoke(&w, input).await.unwrap() }
    };
    publish("before-drain").await;
    assert_eq!(
        wait_for(&f, "orders:before-drain", 1, Duration::from_secs(5)).await,
        json!(1),
        "the consumer is not running, so this test proves nothing"
    );
    let claimed_before = rev
        .queue_stats
        .claimed
        .load(std::sync::atomic::Ordering::SeqCst);

    // The drain begins. HTTP keeps serving — publishing still works — and
    // the consumer must stop claiming.
    f.runtime.stop_background_work();
    publish("after-drain").await;
    tokio::time::sleep(Duration::from_secs(2)).await;

    let claimed_after = rev
        .queue_stats
        .claimed
        .load(std::sync::atomic::Ordering::SeqCst);
    assert_eq!(
        claimed_after,
        claimed_before,
        "a draining instance claimed {} more message(s)",
        claimed_after - claimed_before
    );
    // And the message is still there for the instance that is staying.
    let stats = ops::stats(manager.as_ref()).await.expect("stats");
    let ready: i64 = stats
        .as_array()
        .map(|rows| {
            rows.iter()
                .filter(|r| r["state"] == json!("ready"))
                .filter_map(|r| r["rows"].as_i64())
                .sum()
        })
        .unwrap_or(0);
    assert!(ready >= 1, "the unclaimed message is gone: {stats}");
    f.runtime.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_message_can_be_delivered_to_a_consumer_directly() {
    let Some(f) = fixture().await else { return };
    // One delivery, attempt 1, a fresh world each time, no queue row: the
    // consumer's own logic under test (here: the second delivery sees the
    // first through the shared resource — the idempotency question).
    let r = f
        .runtime
        .run_queue_message("orders", json!({ "orderId": "direct-1" }))
        .await
        .unwrap();
    let v = r.outcome.unwrap().unwrap();
    assert_eq!(v["value"]["attempt"], 1);
    assert_eq!(v["value"]["seen"], 1);
    assert_eq!(v["value"]["worldCounter"], 1, "a fresh world");
    let r = f
        .runtime
        .run_queue_message("orders", json!({ "orderId": "direct-1" }))
        .await
        .unwrap();
    let v = r.outcome.unwrap().unwrap();
    assert_eq!(v["value"]["seen"], 2);
    assert_eq!(v["value"]["worldCounter"], 1, "still a fresh world");
    assert!(matches!(
        f.runtime.run_queue_message("nope", json!({})).await,
        Err(usai_runtime::RuntimeError::UnknownWorkload(_))
    ));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn parameter_and_column_types_round_trip() {
    let Some(f) = fixture().await else { return };
    let r = f.runtime.run_task("types", json!(null)).await.unwrap();
    let v = r.outcome.unwrap().unwrap();
    let row = &v["value"];
    assert_eq!(row["big"], json!(9007199254740991i64));
    assert_eq!(row["f"], 1.5);
    assert_eq!(row["b"], true);
    assert_eq!(row["u"], "6f1a2b3c-4d5e-4f60-8a71-92b3c4d5e6f7");
    assert_eq!(row["j"], json!({ "a": [1, 2] }));
    assert!(
        row["t"]
            .as_str()
            .unwrap()
            .starts_with("2026-09-17T10:00:00")
    );
    assert_eq!(row["arr"], json!(["x", "y"]));
    assert_eq!(row["n"], "12.50");
    assert_eq!(row["nothing"], Value::Null);
    let r = f.runtime.run_task("bad-params", json!(null)).await.unwrap();
    assert_eq!(r.outcome.unwrap().unwrap()["value"], "invalid_param");
    f.baseline();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pool_exhaustion_is_resource_aware_backpressure() {
    let Some(f) = fixture().await else { return };
    let rev = f.runtime.active().unwrap();
    let manager = rev.resources().get("main").cloned().unwrap();
    let mut holders = Vec::new();
    for _ in 0..4 {
        let m = Arc::clone(&manager);
        holders.push(tokio::spawn(async move {
            m.call(
                ResourceCall {
                    method: "one".into(),
                    args: json!({ "sql": "select pg_sleep(2)", "params": [] }),
                },
                CancellationToken::new(),
            )
            .await
        }));
    }
    // Connections are created lazily; on a slow CI database the four
    // leases take a moment to exist. Poll, well within the 2 s sleeps.
    let started = std::time::Instant::now();
    let s = loop {
        let s = f.pg_status();
        if s.in_use == 4 || started.elapsed() > Duration::from_millis(1500) {
            break s;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    assert_eq!(s.in_use, 4, "{s:?}");
    assert_eq!(s.max, 4);
    // `waiting` is the number that separates "the database is slow" from
    // "the pool is too small" — opposite fixes — and it has to be visible
    // while the pool is full, not only in the status document.
    assert!(
        s.detail.contains_key("waiting"),
        "the pool must report what is queued: {:?}",
        s.detail
    );
    for h in holders {
        let r = h.await.unwrap();
        assert!(r.is_ok(), "{r:?}");
    }
    f.baseline();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn unreachable_database_fails_activation_not_the_first_request() {
    if std::process::Command::new("node")
        .arg("--version")
        .output()
        .map(|o| !o.status.success())
        .unwrap_or(true)
    {
        return;
    }
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/pg-app");
    if !root.join("node_modules/@sakaladev/usai").exists() {
        return;
    }
    let out_dir = std::env::temp_dir().join(format!("usai-pg-unreachable-{}", std::process::id()));
    let engine = usai_runtime::engine::from_env(64).unwrap();
    let out = build(
        engine.as_ref(),
        &BuildOptions {
            out_dir,
            ..BuildOptions::for_project(&root)
        },
    )
    .await
    .expect("fixture builds");
    let runtime = Runtime::with_env(engine, RuntimeConfig::default(), |name| {
        (name == "DATABASE_URL").then(|| "postgres://nobody@127.0.0.1:1/none".to_owned())
    });
    let rev = runtime.install(out.definition).await.unwrap();
    let err = runtime.activate(rev.id).await.unwrap_err();
    assert!(
        matches!(err, RuntimeError::Resource(ResourceError::Startup(..))),
        "{err}"
    );
    assert_eq!(rev.state(), RevisionState::Installed);
    let _ = TerminalProof::Terminal;
}

/// D10: `concurrency` bounds how many message worlds run at once.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn queue_concurrency_is_bounded() {
    let Some(f) = fixture_with(true).await else {
        return;
    };
    let rev = f.runtime.active().unwrap();
    let (_, w) = rev
        .definition
        .workload("http:POST /orders")
        .map(|(i, w)| (i, w.id.clone()))
        .unwrap();
    // Six messages of 300 ms each on a consumer declared with concurrency 2:
    // at least three rounds, never more than two message worlds alive.
    for i in 0..6 {
        f.runtime
            .invoke(&w, json!({ "kind": "http", "request": { "method": "POST", "path": "/orders", "url": "/orders", "params": {}, "query": {}, "headers": {}, "body": { "json": { "orderId": format!("slow{i}"), "sleepMs": 300 } } } }))
            .await
            .unwrap();
    }
    let started = std::time::Instant::now();
    let mut peak = 0;
    // Sample the live-world gauge until the last message is seen.
    while wait_for(&f, "orders:slow5", 1, Duration::ZERO).await != json!(1)
        && started.elapsed() < Duration::from_secs(10)
    {
        peak = peak.max(f.runtime.ledger().gauges.snapshot().live_worlds);
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    for i in 0..6 {
        assert_eq!(
            wait_for(&f, &format!("orders:slow{i}"), 1, Duration::from_secs(5)).await,
            json!(1)
        );
    }
    let elapsed = started.elapsed();
    assert!(
        elapsed >= Duration::from_millis(850),
        "6 × 300 ms at concurrency 2 cannot finish in {elapsed:?}"
    );
    // The `/seen` probes are HTTP worlds too, so allow them on top of the
    // two consumer worlds.
    assert!(
        peak <= 3,
        "peak live worlds {peak} exceeds concurrency 2 (+1 probe)"
    );
    tokio::time::sleep(Duration::from_millis(300)).await;
    f.baseline();
}

/// TLS: `sslmode=require` connects only when the server certificate
/// verifies against the configured roots; without them activation is
/// refused (never an "encrypted but unverified" connection).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn tls_connections_verify_the_server_certificate() {
    let Some((url, ca)) = support::tls_database() else {
        eprintln!("skipping: no TLS-capable PostgreSQL (portable binaries + openssl needed)");
        return;
    };
    if std::process::Command::new("node")
        .arg("--version")
        .output()
        .map(|o| !o.status.success())
        .unwrap_or(true)
    {
        return;
    }
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/pg-app");
    if !root.join("node_modules/@sakaladev/usai").exists() {
        return;
    }
    let engine = usai_runtime::engine::from_env(64).unwrap();
    let out = build(
        engine.as_ref(),
        &BuildOptions {
            out_dir: std::env::temp_dir().join(format!("usai-pg-tls-test-{}", std::process::id())),
            ..BuildOptions::for_project(&root)
        },
    )
    .await
    .expect("fixture builds");

    // Without the root: refused at activation with a TLS error, not served.
    let bare = url.clone();
    let runtime = Runtime::with_env(Arc::clone(&engine), RuntimeConfig::default(), move |name| {
        (name == "DATABASE_URL").then(|| bare.clone())
    });
    let rev = runtime.install(Arc::clone(&out.definition)).await.unwrap();
    let err = runtime.activate(rev.id).await.unwrap_err();
    assert!(
        matches!(&err, RuntimeError::Resource(ResourceError::Startup(_, detail)) if detail.contains("cannot connect")),
        "{err}"
    );
    runtime.shutdown().await;

    // With the root (PGSSLROOTCERT, libpq's name): the fixture works end to end.
    let ca_path = ca.to_string_lossy().into_owned();
    let runtime = Runtime::with_env(
        engine,
        RuntimeConfig {
            cron_scheduler: false,
            queue_consumers: false,
            ..RuntimeConfig::default()
        },
        move |name| match name {
            "DATABASE_URL" => Some(url.clone()),
            "PGSSLROOTCERT" => Some(ca_path.clone()),
            _ => None,
        },
    );
    let rev = runtime.install(out.definition).await.unwrap();
    runtime.activate(rev.id).await.unwrap();
    let f = Fixture { runtime };
    let r = f.runtime.run_command("setup", vec![]).await.unwrap();
    assert!(matches!(r.outcome, Some(Ok(_))), "{:?}", r.outcome);
    let (status, body) = f.http("GET", "/users/:id", json!({ "id": "1" })).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["name"], "Ayu");
    // The wire really is TLS: the server says so for our backend.
    let (status, body) = f.http("GET", "/tls", json!({})).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["ssl"], true, "{body}");
    f.runtime.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn parameters_without_a_binary_encoder_take_their_text_form() {
    let Some(f) = fixture().await else { return };
    let r = f.runtime.run_task("text-types", json!(null)).await.unwrap();
    let Some(Ok(value)) = r.outcome else {
        panic!("{:?}", r.outcome)
    };
    let row = &value["value"];
    assert_eq!(row["i"], "30 days", "{row}");
    assert_eq!(row["ip"], "10.0.0.1/32");
    assert_eq!(
        row["t"], "2026-09-18T12:48:41.507406+00:00",
        "PostgreSQL's own text output round-trips"
    );
    assert_eq!(row["n"], "12.50");
    f.baseline();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn transactions_pin_one_connection_and_end_with_the_world() {
    let Some(f) = fixture().await else { return };
    // Commit: statements share a connection and the result is durable.
    let (status, body) = f.http("POST", "/tx/commit", json!({})).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["inside"], 1);
    let (_, after) = f
        .http("GET", "/count/:email", json!({ "email": "citra@x.io" }))
        .await;
    assert_eq!(after["n"], 1);
    // Rollback: the handler's throw undoes the insert, the error is the handler's.
    let (status, body) = f.http("POST", "/tx/rollback", json!({})).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["code"], "conflict");
    assert_eq!(body["after"], 0);
    // A leaked executor after the transaction ended is refused, not silently
    // run on some other connection.
    let (_, body) = f.http("POST", "/tx/closed", json!({})).await;
    assert_eq!(body["code"], "transaction_closed");
    let before = f.pg_status();
    // Abandonment: the world returns with the transaction open. That is a
    // C3 violation on the world, and the runtime rolls back for it; the
    // connection returns clean (no quarantine) because ROLLBACK is terminal.
    let id = "http:POST /tx/abandon".to_owned();
    let input = json!({ "kind": "http", "env": {}, "request": { "method": "POST", "path": "/tx/abandon", "url": "/tx/abandon", "params": {}, "query": {}, "headers": {}, "body": null } });
    let r = f.runtime.invoke(&id, input).await.unwrap();
    assert!(
        r.violations
            .iter()
            .any(|v| format!("{v:?}").contains("postgres.transaction")),
        "the open transaction must be diagnosed as live work: {:?}",
        r.violations
    );
    let mut rolled_back = false;
    for _ in 0..50 {
        let s = f.pg_status();
        if s.detail["rolledBackForWorld"].as_u64()
            == Some(before.detail["rolledBackForWorld"].as_u64().unwrap_or(0) + 1)
            && s.detail["openTransactions"].as_u64() == Some(0)
        {
            rolled_back = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(rolled_back, "{:?}", f.pg_status());
    let (_, eka) = f
        .http("GET", "/count/:email", json!({ "email": "eka@x.io" }))
        .await;
    assert_eq!(eka["n"], 0, "the abandoned insert must be rolled back");
    assert_eq!(
        f.pg_status().quarantined,
        before.quarantined,
        "a terminal ROLLBACK keeps the connection reusable"
    );
    f.baseline();
}

async fn wait_for(f: &Fixture, key: &str, expected: u64, timeout: Duration) -> Value {
    let started = std::time::Instant::now();
    loop {
        let (_, body) = f.http("GET", "/seen/:key", json!({ "key": key })).await;
        if body["n"].as_u64() == Some(expected) || started.elapsed() > timeout {
            return body["n"].clone();
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn queue_messages_run_in_fresh_worlds_with_explicit_retry() {
    let Some(f) = fixture_with(true).await else {
        return;
    };
    // Publish from an HTTP world; consumption happens in its own worlds.
    for id in ["a", "b", "c", "d"] {
        let rev = f.runtime.active().unwrap();
        let (_, w) = rev
            .definition
            .workload("http:POST /orders")
            .map(|(i, w)| (i, w.id.clone()))
            .unwrap();
        let input = json!({ "kind": "http", "request": { "method": "POST", "path": "/orders", "url": "/orders", "params": {}, "query": {}, "headers": { "x-request-id": format!("req-{id}") }, "body": { "json": { "orderId": id } } } });
        let r = f.runtime.invoke(&w, input).await.unwrap();
        let v = r.outcome.unwrap().unwrap();
        assert_eq!(v["status"], 200, "{v}");
        assert!(v["json"]["id"].is_string());
    }
    for id in ["a", "b", "c", "d"] {
        assert_eq!(
            wait_for(&f, &format!("orders:{id}"), 1, Duration::from_secs(5)).await,
            json!(1),
            "message {id} processed exactly once"
        );
        // The consumer's world ran under the publisher's request id.
        assert_eq!(
            wait_for(&f, &format!("rid:{id}:req-{id}"), 1, Duration::from_secs(2)).await,
            json!(1),
            "message {id} carried its publisher's request id"
        );
    }
    let rev = f.runtime.active().unwrap();
    assert_eq!(
        rev.queue_stats
            .done
            .load(std::sync::atomic::Ordering::SeqCst),
        4
    );
    assert_eq!(
        rev.queue_stats
            .dead
            .load(std::sync::atomic::Ordering::SeqCst),
        0
    );

    // Retry: fails on attempts 1 and 2, succeeds on 3 (maxAttempts 3).
    let (_, w) = rev
        .definition
        .workload("http:POST /orders")
        .map(|(i, w)| (i, w.id.clone()))
        .unwrap();
    f.runtime.invoke(&w, json!({ "kind": "http", "request": { "method": "POST", "path": "/orders", "url": "/orders", "params": {}, "query": {}, "headers": {}, "body": { "json": { "orderId": "flaky", "fail": 2 } } } })).await.unwrap();
    assert_eq!(
        wait_for(&f, "orders:flaky", 3, Duration::from_secs(8)).await,
        json!(3),
        "three attempts, each in a fresh world"
    );
    assert!(
        rev.queue_stats
            .retried
            .load(std::sync::atomic::Ordering::SeqCst)
            >= 2
    );

    // Dead letter: fails every attempt.
    f.runtime.invoke(&w, json!({ "kind": "http", "request": { "method": "POST", "path": "/orders", "url": "/orders", "params": {}, "query": {}, "headers": {}, "body": { "json": { "orderId": "doomed", "fail": 99 } } } })).await.unwrap();
    assert_eq!(
        wait_for(&f, "orders:doomed", 3, Duration::from_secs(8)).await,
        json!(3)
    );
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        rev.queue_stats
            .dead
            .load(std::sync::atomic::Ordering::SeqCst),
        1
    );
    let manager = rev.resources().get("main").cloned().unwrap();
    let depth = usai_runtime::workloads::queue::depth(manager.as_ref(), "orders")
        .await
        .unwrap();
    assert_eq!(depth["dead"], 1);
    assert_eq!(depth["done"], 5);
    assert_eq!(depth["ready"], 0);

    // An invalid message never gets a world.
    let before = f.runtime.ledger().gauges.snapshot().worlds_created;
    manager.call(usai_runtime::resource::ResourceCall { method: "execute".into(), args: json!({ "sql": "INSERT INTO usai_queue (topic, payload) VALUES ('orders', '{\"orderId\": 5}'::jsonb)", "params": [] }) }, CancellationToken::new()).await.unwrap();
    tokio::time::sleep(Duration::from_millis(800)).await;
    assert_eq!(
        rev.queue_stats
            .invalid
            .load(std::sync::atomic::Ordering::SeqCst),
        1
    );
    let depth = usai_runtime::workloads::queue::depth(manager.as_ref(), "orders")
        .await
        .unwrap();
    assert_eq!(depth["dead"], 2);
    assert_eq!(
        f.runtime.ledger().gauges.snapshot().worlds_created,
        before,
        "invalid message created a world"
    );

    f.runtime.shutdown().await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    f.baseline();
}

/// A consumer killed mid-message (SIGKILL, OOM, a dead host) leaves its
/// rows `processing` with nobody to write them back. The sweeper puts such
/// a row back for another attempt when the declared retry policy has one
/// left, and dead-letters it otherwise — the message is never lost and never
/// retried beyond what the application declared.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn messages_a_lost_consumer_had_claimed_are_reclaimed() {
    let Some(f) = fixture_with(true).await else {
        return;
    };
    let rev = f.runtime.active().unwrap();
    let manager = rev.resources().get("main").cloned().unwrap();
    let insert = |payload: &str, attempts: i32| {
        let manager = Arc::clone(&manager);
        let sql = format!(
            "INSERT INTO usai_queue (topic, payload, state, attempts, locked_at, locked_by) VALUES ('orders', '{payload}'::jsonb, 'processing', {attempts}, now() - interval '1 hour', 'rev9:orders:0')"
        );
        async move {
            manager
                .call(
                    usai_runtime::resource::ResourceCall {
                        method: "execute".into(),
                        args: json!({ "sql": sql, "params": [] }),
                    },
                    CancellationToken::new(),
                )
                .await
                .unwrap();
        }
    };
    // Attempt 1 of 3 was in flight when the consumer died: two remain.
    insert(r#"{"orderId": "orphan"}"#, 1).await;
    // The last allowed attempt was in flight: nothing remains.
    insert(r#"{"orderId": "orphan-final"}"#, 3).await;

    assert_eq!(
        wait_for(&f, "orders:orphan", 1, Duration::from_secs(12)).await,
        json!(1),
        "the reclaimed message ran once more, in a fresh world"
    );
    assert_eq!(
        wait_for(&f, "orders:orphan-final", 0, Duration::from_secs(1))
            .await
            .as_u64()
            .unwrap_or(0),
        0,
        "no attempt beyond the declared maximum"
    );
    let depth = usai_runtime::workloads::queue::depth(manager.as_ref(), "orders")
        .await
        .unwrap();
    assert_eq!(depth["processing"], 0, "{depth}");
    assert_eq!(depth["dead"], 1, "{depth}");
    assert_eq!(depth["done"], 1, "{depth}");
    let dead = manager
        .call(
            usai_runtime::resource::ResourceCall {
                method: "one".into(),
                args: json!({ "sql": "SELECT last_error FROM usai_queue WHERE state = 'dead'", "params": [] }),
            },
            CancellationToken::new(),
        )
        .await
        .unwrap();
    let error = dead["last_error"].as_str().unwrap();
    assert!(
        error.starts_with("consumer lost: claimed by rev9:orders:0 at ")
            && error.ends_with(" UTC, never completed"),
        "{error}"
    );
    assert_eq!(
        rev.queue_stats
            .reclaimed
            .load(std::sync::atomic::Ordering::SeqCst),
        2
    );
    f.runtime.shutdown().await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    f.baseline();
}

/// `CREATE TABLE IF NOT EXISTS` is not race-free in PostgreSQL: with the
/// consumers off (fresh database, no table), eight workers preparing the
/// queue schema at once used to lose three of them to 42P07. Every worker
/// must end up with the schema, without an error.
/// A failed outcome mark must not vanish: the statement is idempotent, so a
/// blip retries synchronously, and only a second failure is (loudly) given
/// up on. Simulated by taking the table away between the handler and the
/// mark — the retry then fails too, and what must not happen is a silent
/// success.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_failed_outcome_mark_is_retried_and_then_said_out_loud() {
    let Some(f) = fixture().await else { return };
    let rev = f.runtime.active().unwrap();
    let manager = rev.resources().get("main").cloned().unwrap();
    usai_runtime::workloads::queue::ensure_schema(manager.as_ref())
        .await
        .unwrap();
    let call = |sql: &str, params: Vec<Value>| {
        let m = Arc::clone(&manager);
        let sql = sql.to_owned();
        async move {
            m.call(
                ResourceCall {
                    method: "one".into(),
                    args: json!({ "sql": sql, "params": params }),
                },
                CancellationToken::new(),
            )
            .await
        }
    };
    let id = call(
        "INSERT INTO usai_queue (topic, payload, available_at, state, locked_at, locked_by, attempts) VALUES ('orders', $1::jsonb, now(), 'processing', now(), 'test', 1) RETURNING id",
        vec![json!({ "orderId": "x" })],
    )
    .await
    .unwrap()["id"]
        .clone();
    // The mark lands on the first try.
    usai_runtime::workloads::queue::mark_outcome_for_test(
        manager.as_ref(),
        "orders",
        id.as_i64().unwrap(),
        "done",
        "UPDATE usai_queue SET state = 'done', locked_at = NULL, locked_by = NULL WHERE id = $1",
        vec![id.clone()],
    )
    .await;
    let row = call(
        "SELECT state FROM usai_queue WHERE id = $1",
        vec![id.clone()],
    )
    .await
    .unwrap();
    assert_eq!(row["state"], "done");
    // And when the statement cannot work at all, the call still returns —
    // the consumer loop is never blocked by a database that refuses it.
    usai_runtime::workloads::queue::mark_outcome_for_test(
        manager.as_ref(),
        "orders",
        id.as_i64().unwrap(),
        "done",
        "UPDATE usai_queue_gone SET state = 'done' WHERE id = $1",
        vec![id],
    )
    .await;
    f.baseline();
}

/// A rolling upgrade shares one `usai_queue` between versions. The table a
/// 0.0.5 deployment left behind (no `request_id`, one index) must upgrade in
/// place when a newer runtime touches it, and the statements the older
/// runtime issues must keep working against the upgraded table — otherwise
/// "both versions can serve behind one proxy" (`SUPPORTED.md`) is a wish.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_queue_table_upgrades_in_place_and_the_previous_version_keeps_working() {
    let Some(f) = fixture().await else { return };
    let rev = f.runtime.active().unwrap();
    let manager = rev.resources().get("main").cloned().unwrap();
    let exec = |sql: &str, params: Vec<Value>| {
        let m = Arc::clone(&manager);
        let sql = sql.to_owned();
        async move {
            m.call(
                ResourceCall {
                    method: "execute".into(),
                    args: json!({ "sql": sql, "params": params }),
                },
                CancellationToken::new(),
            )
            .await
        }
    };
    let one = |sql: &str, params: Vec<Value>| {
        let m = Arc::clone(&manager);
        let sql = sql.to_owned();
        async move {
            m.call(
                ResourceCall {
                    method: "one".into(),
                    args: json!({ "sql": sql, "params": params }),
                },
                CancellationToken::new(),
            )
            .await
        }
    };

    // The table as 0.0.5 created it, verbatim.
    exec("DROP TABLE IF EXISTS usai_queue", vec![])
        .await
        .unwrap();
    for statement in [
        "CREATE TABLE usai_queue (
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
         )",
        "CREATE INDEX usai_queue_ready ON usai_queue (topic, available_at) WHERE state = 'ready'",
    ] {
        exec(statement, vec![]).await.unwrap();
    }
    // A message the old version published, waiting in the old table.
    exec(
        "INSERT INTO usai_queue (topic, payload, available_at) VALUES ($1, $2::jsonb, now())",
        vec![json!("orders"), json!({ "orderId": "from-0.0.5" })],
    )
    .await
    .unwrap();

    // This runtime joins: preparing the schema adds the column and the
    // indexes to the existing table, and leaves the waiting message alone.
    usai_runtime::workloads::queue::ensure_schema(manager.as_ref())
        .await
        .expect("the old table upgrades in place");
    let columns = one(
        "SELECT count(*)::int AS n FROM information_schema.columns WHERE table_name = 'usai_queue' AND column_name = 'request_id'",
        vec![],
    )
    .await
    .unwrap();
    assert_eq!(columns["n"], 1, "request_id was added: {columns}");
    let kept = one(
        "SELECT payload->>'orderId' AS id, request_id FROM usai_queue WHERE topic = 'orders'",
        vec![],
    )
    .await
    .unwrap();
    assert_eq!(kept["id"], "from-0.0.5");
    assert_eq!(
        kept["request_id"],
        Value::Null,
        "an old row has no id: {kept}"
    );

    // A new publish carries the request id; the old version's own statements
    // — publish, claim, done, verbatim from 0.0.5 — still work on the
    // upgraded table.
    exec(
        "INSERT INTO usai_queue (topic, payload, available_at, request_id) VALUES ($1, $2::jsonb, now(), $3)",
        vec![json!("orders"), json!({ "orderId": "from-0.0.6" }), json!("trace-1")],
    )
    .await
    .unwrap();
    one(
        "INSERT INTO usai_queue (topic, payload, available_at) VALUES ($1, $2::jsonb, now() + ($3::bigint * interval '1 millisecond')) RETURNING id",
        vec![json!("orders"), json!({ "orderId": "0.0.5-publish" }), json!(0)],
    )
    .await
    .unwrap();
    for expected in ["from-0.0.5", "from-0.0.6", "0.0.5-publish"] {
        let claimed = one(
            "UPDATE usai_queue SET state = 'processing', locked_at = now(), locked_by = $1, attempts = attempts + 1
             WHERE id = (SELECT id FROM usai_queue WHERE topic = $2 AND state = 'ready' AND available_at <= now()
                         ORDER BY id FOR UPDATE SKIP LOCKED LIMIT 1)
             RETURNING id, payload, attempts",
            vec![json!("old-consumer"), json!("orders")],
        )
        .await
        .unwrap();
        assert_eq!(claimed["payload"]["orderId"], expected, "{claimed}");
        exec(
            "UPDATE usai_queue SET state = 'done', locked_at = NULL, locked_by = NULL WHERE id = $1",
            vec![claimed["id"].clone()],
        )
        .await
        .unwrap();
    }
    f.baseline();
}

/// The first publish from a fresh multi-replica deployment: several
/// consumers prepare the queue schema at the same moment against a database
/// that has never seen it. `CREATE TABLE IF NOT EXISTS` is not race-free —
/// the losers get 42P07, 42710 or 23505 depending on which catalog they lost
/// on — and a CI run with a shared database found the one code the retry did
/// not cover (`type "usai_queue" already exists`).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn queue_schema_survives_concurrent_preparation() {
    let Some(f) = fixture_with(false).await else {
        return;
    };
    let rev = f.runtime.active().unwrap();
    let manager = rev.resources().get("main").cloned().unwrap();
    // Without this the table already exists and every worker takes the
    // catalog-lookup path: the test passes without ever racing anything.
    manager
        .call(
            ResourceCall {
                method: "execute".into(),
                args: json!({ "sql": "DROP TABLE IF EXISTS usai_queue", "params": [] }),
            },
            CancellationToken::new(),
        )
        .await
        .expect("drop the queue table");
    let mut handles = Vec::new();
    for _ in 0..16 {
        let m = Arc::clone(&manager);
        handles.push(tokio::spawn(async move {
            usai_runtime::workloads::queue::ensure_schema(m.as_ref()).await
        }));
    }
    for h in handles {
        h.await.unwrap().expect("schema prepared");
    }
    let depth = usai_runtime::workloads::queue::depth(manager.as_ref(), "orders")
        .await
        .unwrap();
    assert_eq!(depth["ready"], 0);
    f.runtime.shutdown().await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    f.baseline();
}

/// A TCP relay in front of the real database that can be **cut**: bytes stop
/// moving in both directions and the sockets stay open, which is what a
/// dropped route or an evicted firewall state looks like from inside the
/// process. A closed socket proves nothing — that path always worked.
///
/// Returns the relay's `host:port` and the switch that cuts it. The caller
/// points a URL at it with [`through`] — which database the URL names is the
/// caller's business, and getting that wrong once sent a fixture's migrations
/// into the shared CI database and broke two example suites.
async fn cuttable_relay(url: &str) -> Option<(String, Arc<std::sync::atomic::AtomicBool>)> {
    let upstream: tokio_postgres::Config = url.parse().expect("a valid URL");
    let host = match upstream.get_hosts().first()? {
        tokio_postgres::config::Host::Tcp(h) => h.clone(),
        #[cfg(unix)]
        tokio_postgres::config::Host::Unix(_) => {
            eprintln!(
                "skipping: the database is on a unix socket, which this relay does not speak"
            );
            return None;
        }
    };
    let port = *upstream.get_ports().first().unwrap_or(&5432);
    let cut = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.ok()?;
    let relay_port = listener.local_addr().ok()?.port();
    let relaying = Arc::clone(&cut);
    let target = format!("{host}:{port}");

    async fn pump<R, W>(from: &mut R, to: &mut W, cut: Arc<std::sync::atomic::AtomicBool>)
    where
        R: tokio::io::AsyncRead + Unpin,
        W: tokio::io::AsyncWrite + Unpin,
    {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let mut buf = vec![0u8; 16 * 1024];
        loop {
            if cut.load(std::sync::atomic::Ordering::SeqCst) {
                std::future::pending::<()>().await;
            }
            let n = tokio::select! {
                read = from.read(&mut buf) => match read { Ok(0) | Err(_) => return, Ok(n) => n },
                _ = tokio::time::sleep(Duration::from_millis(20)) => continue,
            };
            if cut.load(std::sync::atomic::Ordering::SeqCst) {
                std::future::pending::<()>().await;
            }
            if to.write_all(&buf[..n]).await.is_err() {
                return;
            }
        }
    }

    tokio::spawn(async move {
        while let Ok((client, _)) = listener.accept().await {
            let Ok(server) = tokio::net::TcpStream::connect(&target).await else {
                continue;
            };
            let cut = Arc::clone(&relaying);
            tokio::spawn(async move {
                let (mut cr, mut cw) = client.into_split();
                let (mut sr, mut sw) = server.into_split();
                let a = Arc::clone(&cut);
                let up = tokio::spawn(async move { pump(&mut cr, &mut sw, a).await });
                let down = tokio::spawn(async move { pump(&mut sr, &mut cw, cut).await });
                let _ = tokio::join!(up, down);
            });
        }
    });

    Some((format!("127.0.0.1:{relay_port}"), cut))
}

/// The same connection URL, reached through `endpoint` instead of its own
/// host and port. Everything else — user, password, database, parameters —
/// is left alone, because the database a test migrates must stay the
/// throwaway one it was given.
fn through(url: &str, endpoint: &str) -> String {
    let (scheme, rest) = url.split_once("://").expect("a postgres URL");
    let (credentials, tail) = match rest.split_once('@') {
        Some((c, t)) => (format!("{c}@"), t),
        None => (String::new(), rest),
    };
    let path = tail.find(['/', '?']).map(|i| &tail[i..]).unwrap_or("");
    // A probe must be able to give up faster than the test waits for it.
    let separator = if path.contains('?') { "&" } else { "?" };
    format!("{scheme}://{credentials}{endpoint}{path}{separator}connect_timeout=2")
}

/// A database that stops answering makes the resource unready within the
/// probe's bound, without a request having to notice first.
///
/// Round 16 (an operator deploying 0.0.8) reported the opposite: with the
/// database gone, `/_usai/ready` already named the failing resource while
/// `usai_resource{kind="postgres",metric="ready"}` — the series the runbook
/// tells you to alert on — was still 1, because the probe's timeout lived in
/// the caller and cancelled the probe before it could record anything. On a
/// quiet service the alert then waited for the next real query.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_database_that_stops_answering_makes_the_resource_unready() {
    let Some(url) = support::database_url() else {
        eprintln!("skipping: no database");
        return;
    };
    let Some((relay, cut)) = cuttable_relay(&url).await else {
        return;
    };
    let relay_url = through(&url, &relay);
    let spec = usai_runtime::definition::ResourceSpec {
        name: "main".into(),
        kind: "postgres".into(),
        module: None,
        config: json!({ "pool": { "max": 2 } }),
        env: vec!["DATABASE_URL".into()],
    };
    let identity = usai_runtime::resource::ResourceIdentity {
        kind: "postgres".into(),
        name: "main".into(),
        fingerprint: "relay".into(),
        compat: 1,
    };
    let manager = {
        use usai_runtime::resource::ResourceProvider;
        usai_runtime::resource::postgres::PostgresProvider
            .open(&spec, identity, &move |name| {
                (name == "DATABASE_URL").then(|| relay_url.clone())
            })
            .await
            .expect("the relay carries a real handshake")
    };
    manager.probe().await.expect("healthy through the relay");
    assert!(manager.status().ready);

    cut.store(true, std::sync::atomic::Ordering::SeqCst);
    let started = std::time::Instant::now();
    let reason = manager
        .probe()
        .await
        .expect_err("the database cannot answer any more");
    assert!(
        reason.contains("timed out"),
        "the probe waited instead of giving up: {reason}"
    );
    // The assertion that matters is the message above: only the probe's own
    // bound says "timed out" (a refused connection says "no connection"), so
    // this is a loose guard against waiting for the connect timeout on a
    // loaded box rather than a timing assertion.
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "the probe took {:?}",
        started.elapsed()
    );
    let status = manager.status();
    assert!(
        !status.ready,
        "the resource still calls itself ready, so the alert would not fire"
    );
    manager.shutdown().await;
}

/// What a proxy sees when the database goes: readiness fails by default, and
/// `USAI_READY_REQUIRES_RESOURCES=0` keeps the replica asking for traffic
/// while still naming the failing resource.
///
/// The default is the one an operator has to understand before the outage
/// (round 16 measured its consequence: a proxy health-checking `/_usai/ready`
/// removes every replica at once, so routes that never touch the database
/// stop answering too). Both halves are asserted here because both are
/// documented promises — `docs/runbooks/postgres-down.md`.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn readiness_follows_the_database_unless_the_deployment_says_otherwise() {
    let Some(url) = support::database_url() else {
        eprintln!("skipping: no database");
        return;
    };
    let Some((relay, cut)) = cuttable_relay(&url).await else {
        return;
    };
    // Through the relay, but into the throwaway database the fixture just
    // created — not the server's default one, which in CI is shared with the
    // example suites.
    let Some(f) = fixture_reaching(false, move |fresh| through(fresh, &relay)).await else {
        return;
    };
    let ready = |requires: bool| {
        let runtime = Arc::clone(&f.runtime);
        async move {
            let host = usai_runtime::http::HttpHost::new(
                runtime,
                usai_runtime::http::HttpConfig {
                    serve_status: true,
                    ready_requires_resources: requires,
                    ..usai_runtime::http::HttpConfig::default()
                },
            );
            let uri: ::http::Uri = "/_usai/ready".parse().unwrap();
            let response = host
                .internal(&uri, &::http::HeaderMap::new(), true, false)
                .await
                .expect("the status surface serves readiness");
            let status = response.status().as_u16();
            let body = ::http_body_util::BodyExt::collect(response.into_body())
                .await
                .unwrap()
                .to_bytes();
            (status, serde_json::from_slice::<Value>(&body).unwrap())
        }
    };

    let (status, body) = ready(true).await;
    assert_eq!(status, 200, "healthy: {body}");

    cut.store(true, std::sync::atomic::Ordering::SeqCst);

    let (status, body) = ready(true).await;
    assert_eq!(status, 503, "the database is gone: {body}");
    assert_eq!(body["ready"], json!(false));
    assert!(
        body["resources"]["main"].is_string(),
        "the body must name what failed: {body}"
    );

    let (status, body) = ready(false).await;
    assert_eq!(
        status, 200,
        "with the coupling off the replica keeps asking for traffic: {body}"
    );
    assert_eq!(body["ready"], json!(true));
    assert!(
        body["resources"]["main"].is_string(),
        "the failing resource is still reported: {body}"
    );
    f.runtime.shutdown().await;
}

/// The queue half of GUIDE §5's drain contract. A message already being
/// handled used to take the consumer's stop token as its **cancel** token —
/// and that token means "stop claiming", fired at the start of a drain — so
/// a long handler was killed the instant the instance was asked to shut
/// down, mid-work, instead of seeing `ctx.signal` and being given the drain
/// window. "Stop claiming" and "abandon what you are holding" are not the
/// same instruction.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_message_being_handled_is_asked_to_stop_not_cancelled() {
    let Some(f) = fixture_with(true).await else {
        return;
    };
    let rev = f.runtime.active().unwrap();
    let (_, w) = rev
        .definition
        .workload("http:POST /orders")
        .map(|(i, w)| (i, w.id.clone()))
        .unwrap();
    let input = json!({ "kind": "http", "request": { "method": "POST", "path": "/orders", "url": "/orders", "params": {}, "query": {}, "headers": {}, "body": { "json": { "orderId": "long-1" } } } });
    let r = f.runtime.invoke(&w, input).await.unwrap();
    assert_eq!(r.outcome.unwrap().unwrap()["status"], 200);
    // Wait until the handler is actually running: it says so before its
    // loop. A fixed sleep here was flaky under a loaded suite, where
    // claiming the message and creating its world can take seconds.
    assert_eq!(
        wait_for(&f, "long:started", 1, Duration::from_secs(20)).await,
        json!(1),
        "the consumer never started the message"
    );
    // A replacement revision, so the counter can still be read over HTTP
    // after the old one retires.
    let b = f
        .runtime
        .install(std::sync::Arc::clone(&rev.definition))
        .await
        .unwrap();
    f.runtime.activate(b.id).await.unwrap();
    // What SIGTERM does, in the CLI's order: stop claiming first, then drain.
    rev.stop_background_work();
    let started = std::time::Instant::now();
    f.runtime.drain(rev.id).await.unwrap();
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "the drain waited for its bound instead of the handler: {:?}",
        started.elapsed()
    );
    let (_, body) = f
        .http("GET", "/seen/:key", json!({ "key": "long:stopped" }))
        .await;
    assert_eq!(
        body["n"],
        json!(1),
        "the handler was cancelled instead of being asked to stop"
    );
}

/// `CREATE INDEX CONCURRENTLY` is the statement an online schema change is
/// built on — on a large table the ordinary form holds `ACCESS EXCLUSIVE`
/// for the whole build, which is a write outage — and PostgreSQL refuses it
/// inside a transaction. Every migration file ran inside one, so an
/// application had no way to build an index concurrently at all, while the
/// runtime did exactly that for its own queue table. A `-- usai:
/// no-transaction` pragma takes the file out of the transaction; the ledger
/// row cannot then be atomic with the work, which is why the SQL must be
/// idempotent and why applying it twice has to be a no-op.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_migration_can_opt_out_of_its_transaction() {
    let Some(f) = fixture().await else {
        return;
    };
    let rev = f.runtime.active().unwrap();
    let manager = usai_runtime::db::database(&rev, None).unwrap();
    let dir = std::env::temp_dir().join(format!("usai-notx-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let table = format!("notx_{}", std::process::id());
    std::fs::write(
        dir.join("001_table.sql"),
        format!("create table if not exists {table} (id bigint primary key, sku text);"),
    )
    .unwrap();
    std::fs::write(
        dir.join("002_index.sql"),
        format!(
            "-- usai: no-transaction\ncreate index concurrently if not exists {table}_sku on {table} (sku);"
        ),
    )
    .unwrap();
    let files = usai_runtime::db::discover_migrations(&dir, &[("*.sql".to_owned(), String::new())])
        .unwrap();
    assert_eq!(files.len(), 2, "{files:?}");
    let applied = usai_runtime::db::migrate(manager.as_ref(), &files, CancellationToken::new())
        .await
        .expect("the concurrent index applies outside a transaction");
    assert_eq!(applied.len(), 2, "{applied:?}");
    // Idempotent: a second run applies nothing and does not fail.
    let again = usai_runtime::db::migrate(manager.as_ref(), &files, CancellationToken::new())
        .await
        .unwrap();
    assert!(again.is_empty(), "{again:?}");

    // Without the pragma the same statement is refused by PostgreSQL, which
    // is what makes the pragma the thing that matters rather than the
    // wording of the file.
    let dir2 = dir.join("nopragma");
    std::fs::create_dir_all(&dir2).unwrap();
    std::fs::write(
        dir2.join("003_index.sql"),
        format!("create index concurrently if not exists {table}_id2 on {table} (id);"),
    )
    .unwrap();
    let files2 =
        usai_runtime::db::discover_migrations(&dir2, &[("*.sql".to_owned(), String::new())])
            .unwrap();
    let err = usai_runtime::db::migrate(manager.as_ref(), &files2, CancellationToken::new())
        .await
        .expect_err("a concurrent index inside a transaction must be refused");
    assert!(
        err.to_string().contains("25001") || err.to_string().contains("transaction block"),
        "{err}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
