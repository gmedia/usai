//! Engineering profile: per-world cost by bundle composition. Release, ignored.
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use usai_runtime::build::{BuildOptions, build};
use usai_runtime::engine::{Engine, RefusingBindings};
use usai_runtime::*;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore]
async fn per_world_cost_by_bundle() {
    let engine = QuickJsEngine::new(QuickJsConfig::default());
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
        let compiled = engine.compile(&out.definition).await.unwrap();
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
