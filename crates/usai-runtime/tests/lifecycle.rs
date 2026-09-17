//! D1 acceptance: lifecycle semantics testable without HTTP.
//!
//! The JS fixture below implements the minimal `__usai_sdk` ABI directly, so
//! these tests hold regardless of what the TypeScript SDK does on top.

use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;
use usai_runtime::definition::*;
use usai_runtime::engine::GuestError;
use usai_runtime::*;

const FIXTURE: &str = r#"
const cache = {
  call: async (name, method, args) => JSON.parse(await __usai.op("resource", JSON.stringify({ name, method, args }))),
};
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
globalThis.__counter = 0;

const workloads = {
  "task:count": async (input) => {
    globalThis.__counter += 1;
    return { counter: globalThis.__counter, echo: input };
  },
  "task:cache-increment": async () => ({ value: await cache.call("hits", "increment", { key: "k" }) }),
  "task:sleep": async (input) => { await sleep(input.ms); return { slept: input.ms }; },
  "task:detach": async () => { setTimeout(() => { globalThis.__counter = 99; }, 1000); return { returned: true }; },
  "task:interval": async () => { setInterval(() => {}, 10); return { returned: true }; },
  "task:throw": async () => { const e = new Error("no such user"); e.usai = { code: "not_found", status: 404 }; throw e; },
  "task:spin": async () => { for (;;) {} },
  "task:log": async () => { console.log("hello", { a: 1 }); return null; },
  "task:cleared": async () => { const t = setTimeout(() => {}, 1000); clearTimeout(t); return { ok: true }; },
  "task:awaited": async () => { let hit = false; await new Promise((r) => setTimeout(() => { hit = true; r(); }, 5)); return { hit }; },
  "task:unknown-op": async () => { try { await __usai.op("nope", ""); return "resolved"; } catch (e) { return { code: e.usai && e.usai.code }; } },
  "task:cancel-aware": async () => {
    let reason = null;
    __usai.onCancel((r) => { reason = r; });
    try { await sleep(10000); } catch (e) { return { caught: e.usai.code, reason }; }
    return { caught: null };
  },
};
const ids = Object.keys(workloads);
globalThis.__usai_sdk = {
  invoke(app, index, inputJson) {
    return app.workloads[index](JSON.parse(inputJson));
  },
};
globalThis.__usai_app = { workloads: ids.map((id) => workloads[id]), ids };
"#;

fn task(id: &str) -> WorkloadSpec {
    WorkloadSpec {
        id: id.into(),
        name: id.trim_start_matches("task:").into(),
        module: None,
        trigger: Trigger::Task,
        contracts: Contracts::default(),
        errors: vec![],
        auth: None,
        resources: vec![],
        dispatches: vec![],
        max_concurrency: None,
        timeout_ms: None,
    }
}

fn workload_ids() -> Vec<&'static str> {
    vec![
        "task:count",
        "task:cache-increment",
        "task:sleep",
        "task:detach",
        "task:interval",
        "task:throw",
        "task:spin",
        "task:log",
        "task:cleared",
        "task:awaited",
        "task:unknown-op",
        "task:cancel-aware",
    ]
}

fn definition(name: &str) -> Arc<ApplicationDefinition> {
    let code = Code::new(FIXTURE);
    let mut workloads: Vec<WorkloadSpec> = workload_ids().into_iter().map(task).collect();
    workloads[2].timeout_ms = None;
    let manifest = Manifest {
        manifest_version: MANIFEST_VERSION,
        name: name.into(),
        modules: vec![],
        workloads,
        resources: vec![ResourceSpec {
            name: "hits".into(),
            kind: "cache.local".into(),
            module: None,
            config: json!({}),
            env: vec![],
        }],
        auth: vec![],
        env: vec![],
        code_sha256: code.sha256.clone(),
    };
    ApplicationDefinition::new(manifest, code).unwrap()
}

fn config() -> RuntimeConfig {
    RuntimeConfig {
        max_worlds: 64,
        default_app_concurrency: 64,
        default_timeout: Duration::from_secs(5),
        cpu_slice: Duration::from_millis(300),
        drain_timeout: Duration::from_secs(5),
        ..RuntimeConfig::default()
    }
}

async fn runtime() -> Arc<Runtime> {
    let engine = usai_runtime::engine::from_env(64).unwrap();
    let rt = Runtime::with_env(engine, config(), |_| None);
    let rev = rt.install(definition("t")).await.unwrap();
    rt.activate(rev.id).await.unwrap();
    rt
}

fn value(result: &WorkResult) -> &Value {
    result.outcome.as_ref().unwrap().as_ref().unwrap()
}

fn assert_baseline(rt: &Runtime) {
    let g = rt.ledger().gauges.snapshot();
    assert_eq!(
        g.live_worlds, 0,
        "live worlds must return to baseline: {g:?}"
    );
    assert_eq!(g.live_ops, 0, "live ops must return to baseline: {g:?}");
    assert_eq!(rt.ledger().live_records(), 0);
    assert_eq!(rt.status().worlds_in_use, 0);
}

#[tokio::test]
async fn world_a_state_is_not_visible_in_world_b() {
    let rt = runtime().await;
    let a = rt.invoke("task:count", json!("a")).await.unwrap();
    let b = rt.invoke("task:count", json!("b")).await.unwrap();
    assert_eq!(value(&a)["counter"], 1);
    assert_eq!(
        value(&b)["counter"],
        1,
        "world B inherited world A's mutable state"
    );
    assert_eq!(value(&b)["echo"], "b");
    assert_ne!(a.world, b.world);
    assert_baseline(&rt);
}

#[tokio::test]
async fn persistent_resource_survives_world_destruction() {
    let rt = runtime().await;
    for expected in 1..=3 {
        let r = rt
            .invoke("task:cache-increment", json!(null))
            .await
            .unwrap();
        assert_eq!(value(&r)["value"], expected);
        assert_eq!(r.completions_delivered, 1);
    }
    assert_baseline(&rt);
    assert_eq!(rt.status().resources.len(), 1);
}

#[tokio::test]
async fn awaited_timer_is_owned_and_delivered() {
    let rt = runtime().await;
    let r = rt.invoke("task:awaited", json!(null)).await.unwrap();
    assert_eq!(value(&r)["hit"], true);
    assert!(r.violations.is_empty());
    assert_baseline(&rt);
}

#[tokio::test]
async fn detached_timer_is_detected_cancelled_and_explained() {
    let rt = runtime().await;
    let r = rt.invoke("task:detach", json!(null)).await.unwrap();
    assert_eq!(value(&r)["returned"], true);
    assert_eq!(r.violations.len(), 1);
    assert_eq!(r.violations[0].code, "detached_work");
    assert!(
        r.violations[0].message.contains("task()"),
        "{}",
        r.violations[0].message
    );
    assert!(r.violations[0].message.contains("1 timer"));
    assert_eq!(rt.ledger().gauges.snapshot().detached_work_detected, 1);
    // The timer's owner is cancelled with the world; it must not linger.
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_baseline(&rt);
}

#[tokio::test]
async fn detached_interval_is_detected() {
    let rt = runtime().await;
    let r = rt.invoke("task:interval", json!(null)).await.unwrap();
    assert_eq!(r.violations.len(), 1);
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_baseline(&rt);
}

#[tokio::test]
async fn cleared_timer_leaves_nothing_behind() {
    let rt = runtime().await;
    let r = rt.invoke("task:cleared", json!(null)).await.unwrap();
    assert!(r.violations.is_empty());
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert_baseline(&rt);
}

#[tokio::test]
async fn cancelled_world_leaves_no_stale_execution_rights() {
    let rt = runtime().await;
    let revision = rt.active().unwrap();
    let admission = rt.admit(&revision, "task:cancel-aware").unwrap();
    let cancel = CancellationToken::new();
    let c = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        c.cancel();
    });
    let r = rt.execute(admission, json!(null), cancel).await.unwrap();
    assert!(
        matches!(r.termination, Termination::Cancelled { .. }),
        "{:?}",
        r.termination
    );
    // The guest saw the cancellation and unwound; its outcome is recorded but
    // the world is still classified as cancelled.
    if let Some(Ok(v)) = &r.outcome {
        assert_eq!(v["caught"], "cancelled");
        assert_eq!(v["reason"], "cancelled by owner");
    }
    assert_eq!(revision.in_flight(), 0);
    assert_baseline(&rt);
}

#[tokio::test]
async fn deadline_ends_the_world_and_ownership_returns() {
    let engine = usai_runtime::engine::from_env(64).unwrap();
    let rt = Runtime::with_env(
        engine,
        RuntimeConfig {
            default_timeout: Duration::from_millis(80),
            ..config()
        },
        |_| None,
    );
    let rev = rt.install(definition("t")).await.unwrap();
    rt.activate(rev.id).await.unwrap();
    let r = rt
        .invoke("task:sleep", json!({ "ms": 10000 }))
        .await
        .unwrap();
    assert!(
        matches!(r.termination, Termination::DeadlineExceeded),
        "{:?}",
        r.termination
    );
    assert!(r.duration < Duration::from_secs(2));
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert_baseline(&rt);
}

#[tokio::test]
async fn thrown_application_error_is_a_contract() {
    let rt = runtime().await;
    let r = rt.invoke("task:throw", json!(null)).await.unwrap();
    let Some(Err(GuestError {
        usai: Some(usai),
        message,
        ..
    })) = &r.outcome
    else {
        panic!("expected a guest error: {:?}", r.outcome);
    };
    assert_eq!(usai["code"], "not_found");
    assert_eq!(usai["status"], 404);
    assert_eq!(message, "no such user");
    assert_baseline(&rt);
}

#[tokio::test]
async fn runaway_synchronous_code_is_interrupted() {
    let rt = runtime().await;
    let r = rt.invoke("task:spin", json!(null)).await.unwrap();
    assert!(
        matches!(r.termination, Termination::Faulted { .. }),
        "{:?}",
        r.termination
    );
    assert!(r.duration < Duration::from_secs(3));
    assert_baseline(&rt);
}

#[tokio::test]
async fn unknown_operation_is_refused_not_hung() {
    let rt = runtime().await;
    let r = rt.invoke("task:unknown-op", json!(null)).await.unwrap();
    assert_eq!(value(&r)["code"], "op_refused");
    assert_baseline(&rt);
}

#[tokio::test]
async fn console_output_is_captured_per_world() {
    let rt = runtime().await;
    let r = rt.invoke("task:log", json!(null)).await.unwrap();
    assert_eq!(r.logs.len(), 1);
    assert_eq!(r.logs[0].message, r#"hello {"a":1}"#);
}

#[tokio::test]
async fn concurrent_worlds_do_not_share_state() {
    let rt = runtime().await;
    let mut handles = Vec::new();
    for i in 0..32u32 {
        let rt = Arc::clone(&rt);
        handles.push(tokio::spawn(async move {
            rt.invoke("task:count", json!(i)).await.unwrap()
        }));
    }
    for (i, h) in handles.into_iter().enumerate() {
        let r = h.await.unwrap();
        assert_eq!(value(&r)["counter"], 1);
        assert_eq!(value(&r)["echo"], i as u32);
    }
    assert_baseline(&rt);
}

#[tokio::test]
async fn admission_is_refused_at_the_boundary_when_budget_is_exhausted() {
    let engine = usai_runtime::engine::from_env(64).unwrap();
    let rt = Runtime::with_env(
        engine,
        RuntimeConfig {
            max_worlds: 1,
            ..config()
        },
        |_| None,
    );
    let rev = rt.install(definition("t")).await.unwrap();
    rt.activate(rev.id).await.unwrap();
    let first = rt.admit(&rev, "task:sleep").unwrap();
    let second = rt.admit(&rev, "task:sleep");
    assert!(
        matches!(second, Err(RuntimeError::Admission(_))),
        "{second:?}"
    );
    drop(first);
    let third = rt.admit(&rev, "task:sleep").unwrap();
    assert_eq!(rev.in_flight(), 1);
    drop(third);
    assert_eq!(rev.in_flight(), 0);
}

#[tokio::test]
async fn revision_replacement_drains_the_old_revision() {
    let rt = runtime().await;
    let a = rt.active().unwrap();
    let b = rt.install(definition("t2")).await.unwrap();
    assert_eq!(b.state(), RevisionState::Installed);
    // Work admitted against A before B activates keeps A alive until it settles.
    let admission = rt.admit(&a, "task:sleep").unwrap();
    let rt2 = Arc::clone(&rt);
    let in_flight = tokio::spawn(async move {
        rt2.execute(admission, json!({ "ms": 150 }), CancellationToken::new())
            .await
            .unwrap()
    });
    rt.activate(b.id).await.unwrap();
    assert_eq!(a.state(), RevisionState::Draining);
    assert_eq!(b.state(), RevisionState::Active);
    assert!(matches!(
        rt.admit(&a, "task:count"),
        Err(RuntimeError::NotActive(..))
    ));
    rt.drain(a.id).await.unwrap();
    assert_eq!(a.state(), RevisionState::Retired);
    let r = in_flight.await.unwrap();
    assert_eq!(value(&r)["slept"], 150);
    let r = rt.invoke("task:count", json!(null)).await.unwrap();
    assert_eq!(value(&r)["counter"], 1);
    assert_baseline(&rt);
}

#[tokio::test]
async fn failed_activation_leaves_the_active_revision_untouched() {
    let rt = runtime().await;
    let a = rt.active().unwrap();
    let code = Code::new(FIXTURE);
    let manifest = Manifest {
        manifest_version: MANIFEST_VERSION,
        name: "needs-env".into(),
        modules: vec![],
        workloads: vec![task("task:count")],
        resources: vec![],
        auth: vec![],
        env: vec![EnvRequirement {
            name: "DATABASE_URL".into(),
            kind: "url".into(),
            required: true,
            values: vec![],
        }],
        code_sha256: code.sha256.clone(),
    };
    let b = rt
        .install(ApplicationDefinition::new(manifest, code).unwrap())
        .await
        .unwrap();
    let err = rt.activate(b.id).await.unwrap_err();
    assert!(
        matches!(err, RuntimeError::MissingEnv(ref n) if n == "DATABASE_URL"),
        "{err}"
    );
    assert_eq!(rt.active().unwrap().id, a.id);
    assert_eq!(a.state(), RevisionState::Active);
    assert_eq!(b.state(), RevisionState::Installed);
}

#[tokio::test]
async fn shutdown_cancels_live_work_and_returns_to_baseline() {
    let rt = runtime().await;
    let rt2 = Arc::clone(&rt);
    let running = tokio::spawn(async move {
        rt2.invoke("task:sleep", json!({ "ms": 10000 }))
            .await
            .unwrap()
    });
    tokio::time::sleep(Duration::from_millis(50)).await;
    rt.shutdown().await;
    let r = running.await.unwrap();
    assert!(
        matches!(r.termination, Termination::Cancelled { .. }),
        "{:?}",
        r.termination
    );
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert_baseline(&rt);
}
