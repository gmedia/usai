//! Engineering profile: per-world cost by bundle composition. Release, ignored.
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use usai_runtime::build::{BuildOptions, build};
use usai_runtime::engine::RefusingBindings;
use usai_runtime::*;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore]
async fn per_world_cost_by_bundle() {
    let engine = usai_runtime::engine::from_env(64).unwrap();
    println!(
        "engine: {}",
        usai_runtime::engine::Engine::name(engine.as_ref())
    );
    for (label, root) in [
        ("sdk only", "/tmp/claude-1000/sdkonly"),
        ("zod/mini", "/tmp/claude-1000/zodmini"),
        ("zod", "examples/hello"),
    ] {
        let root = if root.starts_with('/') {
            PathBuf::from(root)
        } else {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../..")
                .join(root)
        };
        let out = build(
            engine.as_ref(),
            &BuildOptions {
                out_dir: std::env::temp_dir().join(format!("usai-pb-{label}")),
                ..BuildOptions::for_project(&root)
            },
        )
        .await
        .unwrap();
        let compiled = usai_runtime::engine::Engine::compile(engine.as_ref(), &out.definition)
            .await
            .unwrap();
        let n = 200;
        let t = Instant::now();
        for _ in 0..n {
            let bindings: Arc<dyn usai_runtime::engine::HostBindings> = Arc::new(RefusingBindings);
            drop(engine.instantiate(&compiled, bindings).await.unwrap());
        }
        println!(
            "{label:>10}: {:.2} ms/world  bundle {} KB",
            t.elapsed().as_secs_f64() * 1000.0 / n as f64,
            out.definition.code().source.len() / 1024
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore]
async fn per_request_cost_without_http() {
    let capacity: u32 = std::env::var("USAI_PROFILE_CAPACITY")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(64);
    let engine = usai_runtime::engine::from_env(capacity).unwrap();
    println!("capacity: {capacity}");
    println!(
        "engine: {}",
        usai_runtime::engine::Engine::name(engine.as_ref())
    );
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/hello");
    let out = build(
        engine.as_ref(),
        &BuildOptions {
            out_dir: std::env::temp_dir().join("usai-pr"),
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
    let input = serde_json::json!({ "kind": "http", "request": { "method": "GET", "path": "/hello/x", "url": "/hello/x", "params": { "name": "x" }, "query": {}, "headers": {}, "body": null } });
    for _ in 0..20 {
        runtime
            .invoke("http:GET /hello/:name", input.clone())
            .await
            .unwrap();
    }
    let n = 300;
    let t = Instant::now();
    for _ in 0..n {
        let r = runtime
            .invoke("http:GET /hello/:name", input.clone())
            .await
            .unwrap();
        assert!(matches!(
            r.termination,
            usai_runtime::Termination::Completed
        ));
    }
    println!(
        "sequential invoke: {:.3} ms/request",
        t.elapsed().as_secs_f64() * 1000.0 / n as f64
    );
    let t = Instant::now();
    let mut handles = Vec::new();
    for _ in 0..16 {
        let rt = Arc::clone(&runtime);
        let input = input.clone();
        handles.push(tokio::spawn(async move {
            for _ in 0..(n / 16) {
                rt.invoke("http:GET /hello/:name", input.clone())
                    .await
                    .unwrap();
            }
        }));
    }
    for h in handles {
        h.await.unwrap();
    }
    println!(
        "16 concurrent: {:.0} requests/s",
        n as f64 / t.elapsed().as_secs_f64()
    );
}

/// Both engines in one process, alternating, best of three: immune to the
/// machine's mood between runs.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore]
async fn engines_side_by_side() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/hello");
    let input = serde_json::json!({ "kind": "http", "request": { "method": "GET", "path": "/hello/x", "url": "/hello/x", "params": { "name": "x" }, "query": {}, "headers": {}, "body": null } });
    let mut runtimes = Vec::new();
    for name in ["quickjs", "wasm"] {
        let engine = usai_runtime::engine::by_name(name, 64).unwrap();
        let out = build(
            engine.as_ref(),
            &BuildOptions {
                out_dir: std::env::temp_dir().join(format!("usai-sbs-{name}")),
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
        runtimes.push((name, runtime));
    }
    let n = 200;
    for _round in 0..3 {
        for (name, runtime) in &runtimes {
            for _ in 0..10 {
                runtime
                    .invoke("http:GET /hello/:name", input.clone())
                    .await
                    .unwrap();
            }
            let t = Instant::now();
            let mut worlds = Vec::new();
            for _ in 0..n {
                let r = runtime
                    .invoke("http:GET /hello/:name", input.clone())
                    .await
                    .unwrap();
                worlds.push(r.duration.as_secs_f64() * 1000.0);
            }
            let total = t.elapsed().as_secs_f64() * 1000.0 / n as f64;
            let world: f64 = worlds.iter().sum::<f64>() / n as f64;
            println!(
                "{name:>8}: {total:.3} ms/request  (world run {world:.3} ms, create+retire {:.3} ms)",
                total - world
            );
        }
    }
}

fn minflt() -> u64 {
    std::fs::read_to_string("/proc/self/stat")
        .ok()
        .and_then(|s| s.split_whitespace().nth(9).and_then(|v| v.parse().ok()))
        .unwrap_or(0)
}

/// Separates interpreter speed from memory effects: a pure CPU loop in a
/// tiny module (no heap image to speak of) vs the hello request, with minor
/// page faults per request.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore]
async fn interpreter_vs_memory() {
    println!(
        "pagemap_scan available: {}",
        wasmtime::PoolingAllocationConfig::is_pagemap_scan_available()
    );
    let cpu = Code::new(
        "globalThis.__usai_sdk = { invoke: async () => { let s = 0; for (let i = 0; i < 3000000; i++) { s = (s + i) | 0; } return { status: 200, headers: {}, json: s }; }, describe: () => ({}) }; globalThis.__usai_app = {};",
    );
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/hello");
    let input = serde_json::json!({ "kind": "http", "request": { "method": "GET", "path": "/hello/x", "url": "/hello/x", "params": { "name": "x" }, "query": {}, "headers": {}, "body": null } });
    for name in ["quickjs", "wasm"] {
        let engine = usai_runtime::engine::by_name(name, 64).unwrap();
        // CPU loop
        let compiled = engine.compile_code(&cpu).await.unwrap();
        let bindings: Arc<dyn usai_runtime::engine::HostBindings> = Arc::new(RefusingBindings);
        let mut world = engine.instantiate(&compiled, bindings).await.unwrap();
        let t = Instant::now();
        world.invoke(0, "{}").await.unwrap();
        let cpu_ms = t.elapsed().as_secs_f64() * 1000.0;
        // hello requests + faults
        let out = build(
            engine.as_ref(),
            &BuildOptions {
                out_dir: std::env::temp_dir().join(format!("usai-ivm-{name}")),
                ..BuildOptions::for_project(&root)
            },
        )
        .await
        .unwrap();
        let runtime = Runtime::with_env(
            Arc::clone(&engine),
            RuntimeConfig {
                cron_scheduler: false,
                queue_consumers: false,
                ..RuntimeConfig::default()
            },
            |_| None,
        );
        let rev = runtime.install(out.definition).await.unwrap();
        runtime.activate(rev.id).await.unwrap();
        for _ in 0..10 {
            runtime
                .invoke("http:GET /hello/:name", input.clone())
                .await
                .unwrap();
        }
        let n = 100;
        let f0 = minflt();
        let t = Instant::now();
        for _ in 0..n {
            runtime
                .invoke("http:GET /hello/:name", input.clone())
                .await
                .unwrap();
        }
        let per = t.elapsed().as_secs_f64() * 1000.0 / n as f64;
        let faults = (minflt() - f0) as f64 / n as f64;
        println!(
            "{name:>8}: cpu loop {cpu_ms:.2} ms | hello {per:.3} ms/request, {faults:.0} minor faults/request"
        );
    }
}
