//! D15: the local control surface an orchestrator drives.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;
use usai_runtime::build::{BuildOptions, build};
use usai_runtime::control::{ControlConfig, ControlHost, serve};
use usai_runtime::*;

async fn setup() -> Option<(
    Arc<Runtime>,
    PathBuf,
    PathBuf,
    String,
    CancellationToken,
    reqwest::Client,
)> {
    if std::process::Command::new("node")
        .arg("--version")
        .output()
        .map(|o| !o.status.success())
        .unwrap_or(true)
    {
        return None;
    }
    let hello = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/hello");
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/http-app");
    if !hello.join("node_modules/usai").exists() || !fixture.join("node_modules/usai").exists() {
        return None;
    }
    let engine = usai_runtime::engine::from_env(64).unwrap();
    let tag = format!(
        "{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let hello_dir = std::env::temp_dir().join(format!("usai-control-hello-{tag}"));
    let fixture_dir = std::env::temp_dir().join(format!("usai-control-fixture-{tag}"));
    let first = build(
        engine.as_ref(),
        &BuildOptions {
            out_dir: hello_dir.clone(),
            ..BuildOptions::for_project(&hello)
        },
    )
    .await
    .unwrap();
    build(
        engine.as_ref(),
        &BuildOptions {
            out_dir: fixture_dir.clone(),
            ..BuildOptions::for_project(&fixture)
        },
    )
    .await
    .unwrap();
    let runtime = Runtime::with_env(
        engine,
        RuntimeConfig {
            cron_scheduler: false,
            drain_timeout: Duration::from_secs(5),
            ..RuntimeConfig::default()
        },
        |_| None,
    );
    let rev = runtime.install(first.definition).await.unwrap();
    runtime.activate(rev.id).await.unwrap();
    let host = ControlHost::new(
        Arc::clone(&runtime),
        ControlConfig {
            addr: ([127, 0, 0, 1], 0).into(),
            token: Some("s3cret".into()),
        },
    )
    .unwrap();
    let shutdown = CancellationToken::new();
    let (tx, rx) = tokio::sync::oneshot::channel();
    let token = shutdown.clone();
    tokio::spawn(async move {
        serve(host, token, |addr| {
            let _ = tx.send(addr);
        })
        .await
        .unwrap()
    });
    let addr = rx.await.unwrap();
    Some((
        runtime,
        hello_dir,
        fixture_dir,
        format!("http://{addr}"),
        shutdown,
        reqwest::Client::new(),
    ))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn orchestrator_lifecycle_install_activate_drain_remove() {
    let Some((runtime, _hello, fixture_dir, base, shutdown, client)) = setup().await else {
        return;
    };
    let auth = |r: reqwest::RequestBuilder| r.bearer_auth("s3cret");

    // Unauthorized without the token.
    let r = client.get(format!("{base}/health")).send().await.unwrap();
    assert_eq!(r.status(), 401);

    let health: Value = auth(client.get(format!("{base}/health")))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(health["ok"], true);
    assert_eq!(health["active"]["application"], "hello");

    // Install the fixture as a second revision.
    let r = auth(client.post(format!("{base}/revisions")))
        .json(&json!({ "artifact": fixture_dir.to_string_lossy() }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 201);
    let installed: Value = r.json().await.unwrap();
    assert_eq!(installed["state"], "installed");
    assert_eq!(installed["application"], "http-fixture");
    let id = installed["id"].as_u64().unwrap();

    // A bad artifact is refused with a clear error.
    let r = auth(client.post(format!("{base}/revisions")))
        .json(&json!({ "artifact": "/nonexistent" }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 422);

    // Activate: the previous revision starts draining.
    let r = auth(client.post(format!("{base}/revisions/rev{id}/activate")))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200, "{}", r.text().await.unwrap());
    let activated: Value = r.json().await.unwrap();
    assert_eq!(activated["state"], "active");
    assert_eq!(activated["previous"], 1);
    let revisions: Value = auth(client.get(format!("{base}/revisions")))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let states: Vec<(u64, String)> = revisions["revisions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            (
                r["id"].as_u64().unwrap(),
                r["state"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    assert!(states.contains(&(1, "draining".into())), "{states:?}");
    assert!(states.contains(&(id, "active".into())));

    // Drain and remove the old one.
    let r = auth(client.post(format!("{base}/revisions/rev1/drain")))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    let revisions: Value = auth(client.get(format!("{base}/revisions")))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        revisions["revisions"].as_array().unwrap().len(),
        1,
        "drain retires and removes"
    );

    // Removing the active revision is refused.
    let r = auth(client.delete(format!("{base}/revisions/rev{id}")))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 409);

    // Install another, then remove it while merely installed.
    let r = auth(client.post(format!("{base}/revisions")))
        .json(&json!({ "artifact": fixture_dir.to_string_lossy() }))
        .send()
        .await
        .unwrap();
    let spare: Value = r.json().await.unwrap();
    let spare_id = spare["id"].as_u64().unwrap();
    let r = auth(client.delete(format!("{base}/revisions/rev{spare_id}")))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    assert_eq!(runtime.status().revisions.len(), 1);

    // Status and stop.
    let status: Value = auth(client.get(format!("{base}/status")))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(status["engine"] == "quickjs" || status["engine"] == "wasm");
    let r = auth(client.post(format!("{base}/stop")))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 202);
    shutdown.cancel();
    runtime.shutdown().await;
}

#[test]
fn non_loopback_bind_requires_a_token() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let engine = usai_runtime::engine::from_env(64).unwrap();
        let rt = Runtime::with_env(engine, RuntimeConfig::default(), |_| None);
        let err = ControlHost::new(
            rt,
            ControlConfig {
                addr: ([0, 0, 0, 0], 0).into(),
                token: None,
            },
        )
        .err()
        .unwrap();
        assert!(err.to_string().contains("USAI_CONTROL_TOKEN"));
    });
}
