//! Engineering profile of per-world cost (`AGENTS.md` §4: profile before
//! blaming the lifecycle). Run with `--release --ignored`.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use usai_runtime::build::{BuildOptions, build};
use usai_runtime::engine::{Engine, RefusingBindings};
use usai_runtime::*;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore]
async fn per_world_cost_breakdown() {
    let engine = QuickJsEngine::new(QuickJsConfig::default());
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/hello");
    let out = build(
        engine.as_ref(),
        &BuildOptions {
            out_dir: std::env::temp_dir().join("usai-profile"),
            ..BuildOptions::for_project(&root)
        },
    )
    .await
    .unwrap();
    let code_len = out.definition.code().source.len();
    let compiled = engine.compile(&out.definition).await.unwrap();

    // Baseline: a module with no dependencies at all.
    let tiny = Code::new(
        "globalThis.__usai_sdk = { invoke: async () => ({ status: 200, headers: {}, json: null }), describe: () => ({}) }; export default {};",
    );
    let tiny_compiled = engine.compile_code(&tiny).await.unwrap();

    let n = 200;
    for (label, c) in [
        ("hello (with zod)", &compiled),
        ("tiny module", &tiny_compiled),
    ] {
        let t = Instant::now();
        for _ in 0..n {
            let bindings: Arc<dyn usai_runtime::engine::HostBindings> = Arc::new(RefusingBindings);
            let world = engine.instantiate(c, bindings).await.unwrap();
            drop(world);
        }
        println!(
            "{label:>18}: instantiate+drop {:.3} ms/world",
            t.elapsed().as_secs_f64() * 1000.0 / n as f64
        );
    }
    let t = Instant::now();
    for _ in 0..n {
        let _rt = rquickjs_probe().await;
    }
    println!(
        "{:>18}: runtime+context only {:.3} ms/world",
        "engine floor",
        t.elapsed().as_secs_f64() * 1000.0 / n as f64
    );
    println!("bundle size: {} KB", code_len / 1024);
}

async fn rquickjs_probe() {
    // The same steps `instantiate` takes before any application code:
    // a fresh runtime and context.
    let engine = QuickJsEngine::new(QuickJsConfig::default());
    let tiny = Code::new("export default {};");
    let compiled = engine.compile_code(&tiny).await.unwrap();
    let bindings: Arc<dyn usai_runtime::engine::HostBindings> = Arc::new(RefusingBindings);
    let _ = engine.instantiate(&compiled, bindings).await;
}
