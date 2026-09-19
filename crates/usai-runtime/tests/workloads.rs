//! D4/D5 acceptance: tasks with explicit ownership transfer, cron, commands.
//! Builds the same fixture as the HTTP tests. Skips without node.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;
use usai_runtime::build::{BuildOptions, build};
use usai_runtime::host_ops::ChildRelation;
use usai_runtime::*;

async fn runtime() -> Option<Arc<Runtime>> {
    runtime_with(false).await
}

async fn runtime_with(cron_scheduler: bool) -> Option<Arc<Runtime>> {
    if std::process::Command::new("node")
        .arg("--version")
        .output()
        .map(|o| !o.status.success())
        .unwrap_or(true)
    {
        return None;
    }
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/http-app");
    if !root.join("node_modules/@sakaladev/usai").exists() {
        return None;
    }
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let out_dir = std::env::temp_dir().join(format!(
        "usai-wl-test-{}-{}",
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
            drain_timeout: Duration::from_secs(10),
            cron_scheduler,
            ..RuntimeConfig::default()
        },
        |name| (name == "UPSTREAM_URL").then(|| "http://127.0.0.1:9/".to_owned()),
    );
    let rev = runtime.install(out.definition).await.unwrap();
    runtime.activate(rev.id).await.unwrap();
    Some(runtime)
}

fn value(result: &WorkResult) -> &Value {
    result.outcome.as_ref().unwrap().as_ref().unwrap()
}

/// Ownership returns to baseline once the runtime has drained: a running
/// service is a live world by design until then.
async fn baseline(rt: &Runtime) {
    rt.shutdown().await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    let g = rt.ledger().gauges.snapshot();
    assert_eq!(g.live_worlds, 0, "{g:?}");
    assert_eq!(g.live_ops, 0, "{g:?}");
}

async fn audit(rt: &Runtime, key: &str) -> Value {
    let rev = rt.active().unwrap();
    let manager = rev.resources().get("audit").cloned().unwrap();
    manager
        .call(
            usai_runtime::resource::ResourceCall {
                method: "get".into(),
                args: json!({ "key": key }),
            },
            CancellationToken::new(),
        )
        .await
        .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn http_dispatches_a_task_and_ends_before_it_runs() {
    let Some(rt) = runtime().await else { return };
    let r = rt.invoke("http:POST /orders", json!({ "kind": "http", "env": {}, "request": { "method": "POST", "path": "/orders", "url": "/orders", "params": {}, "query": {}, "headers": {}, "body": { "json": { "id": "o1" } } } })).await.unwrap();
    let body = &value(&r)["json"];
    assert_eq!(body["owned"]["recorded"], "owned:o1");
    assert_eq!(
        body["owned"]["sawParent"],
        Value::Null,
        "parent world state leaked into the owned task world"
    );
    assert!(
        body["dispatched"]
            .as_str()
            .unwrap()
            .starts_with("task:record#d")
    );
    assert_eq!(r.children.len(), 2);
    assert_eq!(r.children[0].relation, ChildRelation::Owned);
    assert_eq!(r.children[1].relation, ChildRelation::Transferred);
    assert!(
        r.violations.is_empty(),
        "dispatch must not count as detached work: {:?}",
        r.violations
    );
    // The owned task ran before the response; the transferred one runs on its own.
    assert_eq!(audit(&rt, "owned:o1").await, json!(1));
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(audit(&rt, "dispatched:o1").await, json!(1));
    assert_eq!(rt.tasks().status()["completed"], 1);
    baseline(&rt).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn owned_task_failure_reaches_the_parent_as_a_contract() {
    let Some(rt) = runtime().await else { return };
    let r = rt.run_task("invokes-slow", json!(null)).await.unwrap();
    assert!(
        r.duration >= Duration::from_secs(5),
        "invoke must wait for the child"
    );
    let r = rt.invoke("http:GET /bad-dispatch", json!({ "kind": "http", "env": {}, "request": { "method": "GET", "path": "/bad-dispatch", "url": "/bad-dispatch", "params": {}, "query": {}, "headers": {}, "body": null } })).await.unwrap();
    // Dispatching a task that does not exist is answered by name, not by a
    // bare refusal (it was `op_refused` through 0.0.5)…
    assert_eq!(value(&r)["json"]["code"], "unknown_task");
    // …and so is a task whose concurrency is full: `capacity_exhausted`, 503.
    let r = rt.run_task("invokes-single", json!(null)).await.unwrap();
    assert_eq!(
        value(&r)["value"]["second"],
        json!({ "code": "capacity_exhausted", "status": 503 }),
        "{:?}",
        r.outcome
    );
    let r = rt.run_task("failing", json!(null)).await.unwrap();
    let err = r.outcome.unwrap().unwrap_err();
    assert_eq!(err.usai.unwrap()["code"], "conflict");
    baseline(&rt).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cancelling_the_parent_cancels_the_owned_child() {
    let Some(rt) = runtime().await else { return };
    let rev = rt.active().unwrap();
    let admission = rt.admit(&rev, "task:invokes-slow").unwrap();
    let cancel = CancellationToken::new();
    let c = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(200)).await;
        c.cancel();
    });
    let input = json!({ "kind": "task", "env": {}, "input": null });
    let r = rt.execute(admission, input, cancel).await.unwrap();
    assert!(
        matches!(r.termination, Termination::Cancelled { .. }),
        "{:?}",
        r.termination
    );
    assert!(r.duration < Duration::from_secs(2));
    tokio::time::sleep(Duration::from_millis(200)).await;
    baseline(&rt).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn task_input_contract_is_validated() {
    let Some(rt) = runtime().await else { return };
    let r = rt.run_task("record", json!({ "what": 5 })).await.unwrap();
    let err = r.outcome.unwrap().unwrap_err();
    assert_eq!(err.usai.unwrap()["code"], "validation_failed");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn draining_waits_for_dispatched_tasks() {
    let Some(rt) = runtime().await else { return };
    let a = rt.active().unwrap();
    let r = rt.invoke("http:POST /orders", json!({ "kind": "http", "env": {}, "request": { "method": "POST", "path": "/orders", "url": "/orders", "params": {}, "query": {}, "headers": {}, "body": { "json": { "id": "o2" } } } })).await.unwrap();
    assert!(r.outcome.unwrap().is_ok());
    // Re-install the same definition as a new revision and drain the old one.
    let b = rt.install(Arc::clone(&a.definition)).await.unwrap();
    rt.activate(b.id).await.unwrap();
    rt.drain(a.id).await.unwrap();
    assert_eq!(a.state(), RevisionState::Retired);
    assert_eq!(
        audit(&rt, "dispatched:o2").await,
        json!(1),
        "drain returned before the dispatched task settled"
    );
    baseline(&rt).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cron_runs_in_fresh_worlds_and_can_be_invoked_deterministically() {
    let Some(rt) = runtime().await else { return };
    let r = rt.run_cron("nightly").await.unwrap();
    assert_eq!(value(&r)["value"]["ran"], true);
    let r = rt.run_cron("every-second").await.unwrap();
    assert!(
        value(&r)["value"].as_str().unwrap().contains('T'),
        "scheduledAt is RFC 3339"
    );
    baseline(&rt).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cron_scheduler_ticks_and_skips_overlap() {
    let Some(rt) = runtime_with(true).await else {
        return;
    };
    tokio::time::sleep(Duration::from_millis(3200)).await;
    let ticks = audit(&rt, "cron:every-second").await.as_u64().unwrap_or(0);
    assert!(ticks >= 2, "expected at least 2 ticks, got {ticks}");
    let rev = rt.active().unwrap();
    let stats = &rev.cron_stats;
    assert!(
        stats.skipped.load(std::sync::atomic::Ordering::SeqCst) >= 1,
        "overlap=skip should have skipped a tick"
    );
    assert!(audit(&rt, "cron:overlapping").await.as_u64().unwrap_or(0) <= 2);
    // Draining stops the scheduler; no new ticks after.
    let b = rt.install(Arc::clone(&rev.definition)).await.unwrap();
    rt.activate(b.id).await.unwrap();
    rt.drain(rev.id).await.unwrap();
    baseline(&rt).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn invalid_cron_schedule_fails_at_install() {
    let Some(rt) = runtime().await else { return };
    let rev = rt.active().unwrap();
    let mut manifest = rev.definition.manifest().clone();
    for w in &mut manifest.workloads {
        if let usai_runtime::definition::Trigger::Cron { schedule, .. } = &mut w.trigger {
            *schedule = "not a schedule".into();
        }
    }
    let def = ApplicationDefinition::new(manifest, rev.definition.code().clone()).unwrap();
    let err = rt.install(def).await.unwrap_err();
    assert!(matches!(err, RuntimeError::InvalidDefinition(_)), "{err}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn command_runs_in_a_fresh_finite_world() {
    let Some(rt) = runtime().await else { return };
    let r = rt
        .run_command("reconcile", vec!["--dry-run".into()])
        .await
        .unwrap();
    assert_eq!(value(&r)["value"]["args"], json!(["--dry-run"]));
    assert_eq!(audit(&rt, "command:reconcile").await, json!(1));
    baseline(&rt).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn service_state_persists_for_the_service_lifetime_and_stops_gracefully() {
    let Some(rt) = runtime().await else { return };
    let rev = rt.active().unwrap();
    tokio::time::sleep(Duration::from_millis(450)).await;
    let services = rev.services();
    let ledger = services
        .iter()
        .find(|s| s.name == "ledger-sync")
        .expect("ledger-sync service");
    assert_eq!(
        ledger.state,
        usai_runtime::workloads::services::ServiceState::Running
    );
    let iterations = audit(&rt, "service:iterations").await.as_u64().unwrap_or(0);
    assert!(
        iterations >= 3,
        "service loop should have iterated, got {iterations}"
    );
    // The service world's globals are invisible to finite work.
    let r = rt.invoke("http:GET /service-local", json!({ "kind": "http", "request": { "method": "GET", "path": "/service-local", "url": "/service-local", "params": {}, "query": {}, "headers": {}, "body": null } })).await.unwrap();
    assert_eq!(value(&r)["json"]["sees"], Value::Null);
    // Graceful stop: the loop observes the signal and returns normally.
    let b = rt.install(Arc::clone(&rev.definition)).await.unwrap();
    rt.activate(b.id).await.unwrap();
    let started = std::time::Instant::now();
    rt.drain(rev.id).await.unwrap();
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "graceful stop should be prompt"
    );
    let final_count = audit(&rt, "service:final").await.as_u64().unwrap_or(0);
    assert!(
        final_count >= 3,
        "service ran its shutdown path: {final_count}"
    );
    let services = rev.services();
    let ledger = services.iter().find(|s| s.name == "ledger-sync").unwrap();
    assert_eq!(
        ledger.state,
        usai_runtime::workloads::services::ServiceState::Stopped,
        "{services:?}"
    );
    // The new revision started its own service.
    tokio::time::sleep(Duration::from_millis(150)).await;
    let fresh = b.services();
    assert_eq!(
        fresh
            .iter()
            .find(|s| s.name == "ledger-sync")
            .unwrap()
            .state,
        usai_runtime::workloads::services::ServiceState::Running
    );
    baseline(&rt).await;
}
