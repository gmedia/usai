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
    let Some(url) = support::database_url() else {
        eprintln!("skipping: no PostgreSQL available");
        return None;
    };
    // One database per fixture: tests run concurrently.
    let url = support::fresh_database(&url).await;
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
        let input = json!({ "kind": "http", "request": { "method": "POST", "path": "/orders", "url": "/orders", "params": {}, "query": {}, "headers": {}, "body": { "json": { "orderId": id } } } });
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
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn queue_schema_survives_concurrent_preparation() {
    let Some(f) = fixture_with(false).await else {
        return;
    };
    let rev = f.runtime.active().unwrap();
    let manager = rev.resources().get("main").cloned().unwrap();
    let mut handles = Vec::new();
    for _ in 0..8 {
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
