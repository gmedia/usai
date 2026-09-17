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
    if !root.join("node_modules/usai").exists() {
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
    let (status, _) = f.http("GET", "/slow", json!({})).await;
    assert_eq!(status, 504);
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
    if !root.join("node_modules/usai").exists() {
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
