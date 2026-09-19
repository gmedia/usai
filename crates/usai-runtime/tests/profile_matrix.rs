//! P1 execution-path attribution (ADR-0016 follow-up): a workload matrix
//! with a per-phase time ledger, so the per-world cost is *accounted for*
//! before anything is optimized. Release, ignored, engineering only.
//!
//!   USAI_PROFILE=1 USAI_ENGINE=wasm|quickjs [USAI_WASM_CORE=<path>] \
//!     cargo test --release -p usai-runtime --test profile_matrix -- --ignored --nocapture
//!
//! Columns: total = wall time of `Runtime::invoke` per request; run = the
//! driver's run phase (invoke → outcome); create = admission + instantiate;
//! the `engine.*` columns are the guest phases the engine timed itself;
//! unaccounted = total − everything timed. Faults are minor page faults per
//! request from /proc/self/stat.
mod support;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use serde_json::{Value, json};
use usai_runtime::build::{BuildOptions, build};
use usai_runtime::*;

fn http(method: &str, path: &str, params: Value, body: Value) -> Value {
    json!({ "kind": "http", "env": {}, "request": { "method": method, "path": path, "url": path, "params": params, "query": {}, "headers": {}, "body": body } })
}

fn task() -> Value {
    json!({ "kind": "task", "env": {}, "input": null })
}

fn rows() -> Vec<(&'static str, String, Value)> {
    vec![
        ("empty", "task:empty".into(), task()),
        ("constant", "task:constant".into(), task()),
        ("loop 1e5", "task:loop".into(), task()),
        ("objects 5k", "task:objects".into(), task()),
        ("json 200k", "task:json".into(), task()),
        ("host x1", "task:host1".into(), task()),
        ("host x8", "task:host8".into(), task()),
        (
            "sdk-only",
            "http:GET /sdk/:name".into(),
            http("GET", "/sdk/x", json!({ "name": "x" }), Value::Null),
        ),
        (
            "zod/mini",
            "http:GET /mini/:name".into(),
            http("GET", "/mini/x", json!({ "name": "x" }), Value::Null),
        ),
        (
            "zod",
            "http:GET /zod/:name".into(),
            http("GET", "/zod/x", json!({ "name": "x" }), Value::Null),
        ),
        ("zod x1", "task:zod1".into(), task()),
        ("zod x10", "task:zod10".into(), task()),
        ("mini x1", "task:mini1".into(), task()),
        ("mini x10", "task:mini10".into(), task()),
        (
            "crud list",
            "http:GET /todos".into(),
            http("GET", "/todos", json!({}), Value::Null),
        ),
        (
            "crud create",
            "http:POST /todos".into(),
            http(
                "POST",
                "/todos",
                json!({}),
                json!({ "json": { "title": "write the ledger", "done": false, "tags": ["p1"] } }),
            ),
        ),
    ]
}

fn minflt() -> u64 {
    std::fs::read_to_string("/proc/self/stat")
        .ok()
        .and_then(|s| s.split_whitespace().nth(9).and_then(|v| v.parse().ok()))
        .unwrap_or(0)
}

async fn runtime_for(root: &str, engine: Arc<dyn usai_runtime::engine::Engine>) -> Arc<Runtime> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(root);
    let out = build(
        engine.as_ref(),
        &BuildOptions {
            out_dir: std::env::temp_dir().join(format!(
                "usai-matrix-{}-{}",
                usai_runtime::engine::Engine::name(engine.as_ref()),
                root.file_name().unwrap().to_string_lossy()
            )),
            ..BuildOptions::for_project(&root)
        },
    )
    .await
    .unwrap();
    let runtime = Runtime::with_env(
        engine,
        RuntimeConfig {
            cron_scheduler: false,
            queue_consumers: false,
            ..RuntimeConfig::default()
        },
        |_| None,
    );
    let rev = runtime.install(out.definition).await.unwrap();
    runtime.activate(rev.id).await.unwrap();
    runtime
}

struct Row {
    label: &'static str,
    total_ms: f64,
    cpu_ms: f64,
    faults: f64,
    phases: BTreeMap<String, f64>,
}

async fn measure(
    runtime: &Runtime,
    label: &'static str,
    workload: &str,
    input: &Value,
    n: usize,
) -> Row {
    for _ in 0..20 {
        let r = runtime.invoke(workload, input.clone()).await.unwrap();
        assert!(
            matches!(r.termination, Termination::Completed),
            "{label}: {:?} {:?}",
            r.termination,
            r.outcome
        );
        if let Some(Err(e)) = &r.outcome {
            panic!("{label}: guest error {e:?}");
        }
    }
    let mut phases: BTreeMap<String, f64> = BTreeMap::new();
    let mut cpu = std::time::Duration::ZERO;
    let f0 = minflt();
    let t = Instant::now();
    for _ in 0..n {
        let r = runtime.invoke(workload, input.clone()).await.unwrap();
        cpu += r.cpu;
        for (k, v) in r.profile {
            *phases.entry(k).or_default() += v;
        }
    }
    let total_ms = t.elapsed().as_secs_f64() * 1000.0 / n as f64;
    let cpu_ms = cpu.as_secs_f64() * 1000.0 / n as f64;
    let faults = (minflt() - f0) as f64 / n as f64;
    for v in phases.values_mut() {
        *v /= n as f64;
    }
    Row {
        label,
        total_ms,
        cpu_ms,
        faults,
        phases,
    }
}

fn print_table(engine: &str, rows: &[Row]) {
    let mut keys: Vec<String> = rows.iter().flat_map(|r| r.phases.keys().cloned()).collect();
    keys.sort();
    keys.dedup();
    // Leaves only: the driver/runtime totals are containers for engine.* phases.
    let leaves: Vec<&String> = keys.iter().filter(|k| k.starts_with("engine.")).collect();
    println!("\n== {engine} ==  (ms per request)");
    print!(
        "{:<12} {:>7} {:>7} {:>7} {:>7} {:>7} {:>7}",
        "workload", "total", "cpu", "create", "run", "retire", "outside"
    );
    for k in &leaves {
        print!(
            " {:>9}",
            k.trim_start_matches("engine.")
                .replace("instantiate.", "inst.")
        );
    }
    println!(" {:>7} {:>7}", "unacct", "faults");
    for r in rows {
        let create = r.phases.get("runtime.create").copied().unwrap_or(0.0);
        let run = r.phases.get("driver.run").copied().unwrap_or(0.0);
        let retire = r.phases.get("driver.retire").copied().unwrap_or(0.0);
        let engine_sum: f64 = leaves
            .iter()
            .filter(|k| !k.starts_with("engine.instantiate"))
            .map(|k| r.phases.get(*k).copied().unwrap_or(0.0))
            .sum();
        // Run time outside timed guest calls: driver bookkeeping, completion
        // routing, host-op futures, select loops.
        let unacct = run - engine_sum;
        // Wall time outside create/run/retire: dropping the instance (slot
        // reset / runtime teardown) plus `Runtime::invoke` bookkeeping.
        let outside = r.total_ms - create - run - retire;
        print!(
            "{:<12} {:>7.3} {:>7.3} {:>7.3} {:>7.3} {:>7.3} {:>7.3}",
            r.label, r.total_ms, r.cpu_ms, create, run, retire, outside
        );
        for k in &leaves {
            print!(" {:>9.3}", r.phases.get(*k).copied().unwrap_or(0.0));
        }
        println!(" {:>7.3} {:>7.1}", unacct, r.faults);
    }
    println!(
        "(cpu = thread CPU inside guest entries; outside = instance drop/slot reset + invoke bookkeeping; unacct = run outside timed guest calls; faults = minor page faults per request)"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore]
async fn workload_matrix() {
    assert!(
        usai_runtime::engine::profiling(),
        "set USAI_PROFILE=1 so the engines keep their phase ledger"
    );
    let n: usize = std::env::var("USAI_MATRIX_N")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(300);
    // USAI_PROFILE_TRACING=<filter> installs a subscriber writing to a sink,
    // to price observability at a given level (D12: "detailed tracing can
    // be disabled cheaply" is a claim to measure, not assert).
    if let Ok(filter) = std::env::var("USAI_PROFILE_TRACING") {
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_writer(std::io::sink)
            .init();
    }
    let engine = usai_runtime::engine::from_env(64).unwrap();
    let name = usai_runtime::engine::Engine::name(engine.as_ref()).to_string();
    let runtime = runtime_for("tests/fixtures/bench-app", engine).await;
    let only: Option<Vec<String>> = std::env::var("USAI_MATRIX_ROWS")
        .ok()
        .map(|v| v.split(',').map(|s| s.trim().to_string()).collect());
    let mut out = Vec::new();
    for (label, workload, input) in rows() {
        if only.as_ref().is_some_and(|o| !o.iter().any(|x| x == label)) {
            continue;
        }
        out.push(measure(&runtime, label, &workload, &input, n).await);
    }
    let core = std::env::var("USAI_WASM_CORE").unwrap_or_else(|_| "vendored".into());
    print_table(&format!("{name} core={core} n={n}"), &out);
}

/// CRUD + PostgreSQL: the pg fixture's `/users/:id`, when a database is
/// available (USAI_TEST_DATABASE_URL or the portable server).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore]
async fn workload_matrix_postgres() {
    assert!(usai_runtime::engine::profiling(), "set USAI_PROFILE=1");
    let Some(server) = support::database_url() else {
        eprintln!("no database; skipping");
        return;
    };
    let url = support::fresh_database(&server).await;
    let n: usize = std::env::var("USAI_MATRIX_N")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(300);
    let engine = usai_runtime::engine::from_env(64).unwrap();
    let name = usai_runtime::engine::Engine::name(engine.as_ref()).to_string();
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/pg-app");
    let out = build(
        engine.as_ref(),
        &BuildOptions {
            out_dir: std::env::temp_dir().join(format!("usai-matrix-pg-{name}")),
            ..BuildOptions::for_project(&root)
        },
    )
    .await
    .unwrap();
    let runtime = Runtime::with_env(
        engine,
        RuntimeConfig {
            cron_scheduler: false,
            queue_consumers: false,
            ..RuntimeConfig::default()
        },
        move |name| (name == "DATABASE_URL").then(|| url.clone()),
    );
    let rev = runtime.install(out.definition).await.unwrap();
    runtime.activate(rev.id).await.unwrap();
    let setup = runtime.run_command("setup", vec![]).await.unwrap();
    assert!(matches!(setup.outcome, Some(Ok(_))), "{:?}", setup.outcome);
    let input = http("GET", "/users/1", json!({ "id": "1" }), Value::Null);
    let row = measure(&runtime, "crud + pg", "http:GET /users/:id", &input, n).await;
    print_table(&format!("{name} postgres n={n}"), &[row]);
}

/// The whole request: the same rows through the HTTP host (in-process
/// listener, one keep-alive connection, c=1), attributed with the
/// `x-usai-profile` header — host phases (route, decode, validate, admit,
/// execute, encode), the runtime/driver/engine phases and the guest's own
/// ledger (dispatch, validation, handler, response). This is the invoice the
/// P8 parity study reads; `USAI_MATRIX_ROWS` selects rows.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore]
async fn http_invoice() {
    assert!(usai_runtime::engine::profiling(), "set USAI_PROFILE=1");
    let n: usize = std::env::var("USAI_MATRIX_N")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(300);
    let engine = usai_runtime::engine::from_env(64).unwrap();
    let name = usai_runtime::engine::Engine::name(engine.as_ref()).to_string();
    let runtime = runtime_for("tests/fixtures/bench-app", engine).await;
    let host = usai_runtime::http::HttpHost::new(
        Arc::clone(&runtime),
        usai_runtime::http::HttpConfig {
            addr: ([127, 0, 0, 1], 0).into(),
            expose_diagnostics: false,
            ..usai_runtime::http::HttpConfig::default()
        },
    );
    let shutdown = tokio_util::sync::CancellationToken::new();
    let (tx, rx) = tokio::sync::oneshot::channel();
    let token = shutdown.clone();
    tokio::spawn(async move {
        usai_runtime::http::serve(host, token, |addr| {
            let _ = tx.send(addr);
        })
        .await
        .unwrap();
    });
    let addr = rx.await.unwrap();
    let client = reqwest::Client::builder()
        .pool_max_idle_per_host(1)
        .build()
        .unwrap();
    let only: Option<Vec<String>> = std::env::var("USAI_MATRIX_ROWS")
        .ok()
        .map(|v| v.split(',').map(|s| s.trim().to_string()).collect());
    let routes: Vec<(&str, &str)> = vec![
        ("sdk-only", "/sdk/x"),
        ("zod/mini", "/mini/x"),
        ("zod", "/zod/x"),
    ];
    println!("\n== {name} http invoice n={n} (ms per request, c=1, in-process listener) ==");
    for (label, path) in routes {
        if only.as_ref().is_some_and(|o| !o.iter().any(|x| x == label)) {
            continue;
        }
        let url = format!("http://{addr}{path}");
        for _ in 0..20 {
            let r = client.get(&url).send().await.unwrap();
            let status = r.status();
            let body = r.text().await.unwrap_or_default();
            assert_eq!(status, 200, "{label}: {body}");
        }
        let mut phases: BTreeMap<String, f64> = BTreeMap::new();
        let t = Instant::now();
        for _ in 0..n {
            let r = client.get(&url).send().await.unwrap();
            let header = r
                .headers()
                .get("x-usai-profile")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .to_owned();
            let _ = r.bytes().await;
            for item in header.split(',') {
                if let Some((k, v)) = item.split_once('=') {
                    *phases.entry(k.to_owned()).or_default() += v.parse::<f64>().unwrap_or(0.0);
                }
            }
        }
        let total = t.elapsed().as_secs_f64() * 1000.0 / n as f64;
        for v in phases.values_mut() {
            *v /= n as f64;
        }
        let get = |k: &str| phases.get(k).copied().unwrap_or(0.0);
        let host_sum: f64 = ["route", "decode", "validate", "admit", "encode"]
            .iter()
            .map(|k| get(&format!("http.{k}")))
            .sum();
        let execute = get("http.execute");
        let create = get("runtime.create");
        let run = get("driver.run");
        let retire = get("driver.retire");
        let guest: Vec<(&String, &f64)> = phases
            .iter()
            .filter(|(k, _)| k.starts_with("guest."))
            .collect();
        let guest_sum: f64 = guest.iter().map(|(_, v)| **v).sum();
        println!("\n{label}: end-to-end {total:.3} ms (client round trip, same process)");
        println!(
            "  host    route {:.3}  decode {:.3}  validate {:.3}  admit {:.3}  encode {:.3}   = {:.3}",
            get("http.route"),
            get("http.decode"),
            get("http.validate"),
            get("http.admit"),
            get("http.encode"),
            host_sum
        );
        println!(
            "  execute {execute:.3}  = create {create:.3} + run {run:.3} + retire {retire:.3} + release {:.3}",
            execute - create - run - retire
        );
        print!("  engine ");
        for (k, v) in phases.iter().filter(|(k, _)| k.starts_with("engine.")) {
            print!(" {}={v:.3}", k.trim_start_matches("engine."));
        }
        println!();
        print!("  guest  ");
        for (k, v) in &guest {
            print!(" {}={v:.3}", k.trim_start_matches("guest."));
        }
        println!("   = {guest_sum:.3}");
        println!(
            "  unaccounted: run − engine − guest-ledger-overlap is not additive (guest phases lie inside invoke.jobs); client+network = {:.3}",
            total - host_sum - execute
        );
    }
    shutdown.cancel();
}
