//! D13: production-shaped evidence. Each test exercises a failure the
//! runtime must survive with ownership returning to baseline.

mod support;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;
use usai_runtime::build::{BuildOptions, build, load_artifact};
use usai_runtime::http::{HttpConfig, HttpHost, serve};
use usai_runtime::*;

fn fixture_root(name: &str) -> Option<PathBuf> {
    if std::process::Command::new("node")
        .arg("--version")
        .output()
        .map(|o| !o.status.success())
        .unwrap_or(true)
    {
        return None;
    }
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    root.join("node_modules/usai").exists().then_some(root)
}

fn out_dir(tag: &str) -> PathBuf {
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    std::env::temp_dir().join(format!(
        "usai-hard-{tag}-{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    ))
}

async fn http_runtime() -> Option<(Arc<Runtime>, Arc<dyn usai_runtime::engine::Engine>)> {
    let root = fixture_root("http-app")?;
    let engine = usai_runtime::engine::from_env(64).unwrap();
    let out = build(
        engine.as_ref(),
        &BuildOptions {
            out_dir: out_dir("http"),
            ..BuildOptions::for_project(&root)
        },
    )
    .await
    .unwrap();
    let runtime = Runtime::with_env(
        Arc::clone(&engine) as Arc<dyn usai_runtime::engine::Engine>,
        RuntimeConfig {
            cron_scheduler: false,
            default_timeout: Duration::from_secs(10),
            drain_timeout: Duration::from_secs(10),
            ..RuntimeConfig::default()
        },
        |_| None,
    );
    let rev = runtime.install(out.definition).await.unwrap();
    runtime.activate(rev.id).await.unwrap();
    Some((runtime, engine))
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
async fn a_world_exceeding_its_memory_limit_fails_cleanly() {
    let Some((rt, _)) = http_runtime().await else {
        return;
    };
    let r = rt.run_task("memory-hog", json!(null)).await.unwrap();
    match (&r.termination, &r.outcome) {
        (Termination::Faulted { detail }, _) => assert!(
            detail.contains("memory")
                || detail.contains("out of memory")
                || detail.contains("InternalError"),
            "{detail}"
        ),
        (Termination::Completed, Some(Err(e))) => assert!(
            e.name.contains("InternalError") || e.message.contains("memory"),
            "{e:?}"
        ),
        other => panic!("expected a clean failure, got {other:?}"),
    }
    // The runtime is unaffected: the next world works.
    let r = rt.invoke("http:GET /counter", json!({ "kind": "http", "request": { "method": "GET", "path": "/counter", "url": "/counter", "params": {}, "query": {}, "headers": {}, "body": null } })).await.unwrap();
    assert_eq!(r.outcome.unwrap().unwrap()["json"]["counter"], 1);
    rt.shutdown().await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    let g = rt.ledger().gauges.snapshot();
    assert_eq!(g.live_worlds, 0);
    assert_eq!(g.live_ops, 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_failing_service_restarts_per_policy_and_then_settles() {
    let Some((rt, _)) = http_runtime().await else {
        return;
    };
    let started = std::time::Instant::now();
    loop {
        let starts = audit(&rt, "service:crashy:starts")
            .await
            .as_u64()
            .unwrap_or(0);
        if starts >= 3 {
            break;
        }
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "service did not restart in time"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    tokio::time::sleep(Duration::from_millis(100)).await;
    let rev = rt.active().unwrap();
    let crashy = rev
        .services()
        .into_iter()
        .find(|s| s.name == "crashy")
        .unwrap();
    assert_eq!(
        crashy.state,
        usai_runtime::workloads::services::ServiceState::Running,
        "{crashy:?}"
    );
    assert_eq!(crashy.restarts, 2);
    rt.shutdown().await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(rt.ledger().gauges.snapshot().live_worlds, 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn revision_replacement_under_load_loses_no_request() {
    let Some((rt, _)) = http_runtime().await else {
        return;
    };
    let host = HttpHost::new(
        Arc::clone(&rt),
        HttpConfig {
            addr: ([127, 0, 0, 1], 0).into(),
            ..HttpConfig::default()
        },
    );
    let (tx, rx) = tokio::sync::oneshot::channel();
    let token = CancellationToken::new();
    let t = token.clone();
    tokio::spawn(async move {
        serve(host, t, |addr| {
            let _ = tx.send(addr);
        })
        .await
        .unwrap()
    });
    let addr = rx.await.unwrap();
    let client = reqwest::Client::new();
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let mut clients = Vec::new();
    for _ in 0..8 {
        let client = client.clone();
        let stop = Arc::clone(&stop);
        let url = format!("http://{addr}/users/6f1a2b3c-4d5e-4f60-8a71-92b3c4d5e6f7");
        clients.push(tokio::spawn(async move {
            let (mut ok, mut bad) = (0u64, 0u64);
            while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                match client.get(&url).send().await {
                    Ok(r) if r.status() == 200 => ok += 1,
                    Ok(r) => {
                        eprintln!(
                            "unexpected status {}: {}",
                            r.status(),
                            r.text().await.unwrap_or_default()
                        );
                        bad += 1;
                    }
                    Err(e) => {
                        eprintln!("request error during replacement: {e}");
                        bad += 1;
                    }
                }
            }
            (ok, bad)
        }));
    }
    // Three rolling replacements while traffic flows.
    for _ in 0..3 {
        tokio::time::sleep(Duration::from_millis(200)).await;
        let old = rt.active().unwrap();
        let new = rt.install(Arc::clone(&old.definition)).await.unwrap();
        rt.activate(new.id).await.unwrap();
        rt.drain(old.id).await.unwrap();
        assert_eq!(old.state(), RevisionState::Retired);
    }
    tokio::time::sleep(Duration::from_millis(200)).await;
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    let (mut ok, mut bad) = (0, 0);
    for c in clients {
        let (o, b) = c.await.unwrap();
        ok += o;
        bad += b;
    }
    assert!(ok > 50, "expected sustained traffic, got {ok}");
    assert_eq!(bad, 0, "requests failed during revision replacement");
    assert_eq!(rt.status().revisions.len(), 1, "old revisions retired");
    token.cancel();
    rt.shutdown().await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(rt.ledger().gauges.snapshot().live_worlds, 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn malformed_artifacts_are_refused_with_clear_errors() {
    let Some(root) = fixture_root("http-app") else {
        return;
    };
    let engine = usai_runtime::engine::from_env(64).unwrap();
    let dir = out_dir("artifact");
    build(
        engine.as_ref(),
        &BuildOptions {
            out_dir: dir.clone(),
            ..BuildOptions::for_project(&root)
        },
    )
    .await
    .unwrap();
    assert!(load_artifact(&dir).await.is_ok());
    // Tampered code: the manifest no longer describes it.
    let code_path = dir.join("app.js");
    let original = std::fs::read_to_string(&code_path).unwrap();
    std::fs::write(&code_path, format!("{original}\n// tampered\n")).unwrap();
    let err = load_artifact(&dir).await.unwrap_err().to_string();
    assert!(err.contains("hashes to"), "{err}");
    std::fs::write(&code_path, &original).unwrap();
    // Unsupported manifest version.
    let manifest_path = dir.join("manifest.json");
    let mut manifest: Value =
        serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
    manifest["manifestVersion"] = json!(99);
    std::fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    let err = load_artifact(&dir).await.unwrap_err().to_string();
    assert!(err.contains("manifest version 99"), "{err}");
    // Missing files.
    std::fs::remove_file(&manifest_path).unwrap();
    assert!(load_artifact(&dir).await.is_err());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn budget_exhaustion_refuses_promptly_and_recovers() {
    let Some(root) = fixture_root("http-app") else {
        return;
    };
    let engine = usai_runtime::engine::from_env(64).unwrap();
    let out = build(
        engine.as_ref(),
        &BuildOptions {
            out_dir: out_dir("budget"),
            ..BuildOptions::for_project(&root)
        },
    )
    .await
    .unwrap();
    let rt = Runtime::with_env(
        engine,
        RuntimeConfig {
            cron_scheduler: false,
            max_worlds: 4,
            default_timeout: Duration::from_secs(10),
            ..RuntimeConfig::default()
        },
        |_| None,
    );
    let rev = rt.install(out.definition).await.unwrap();
    rt.activate(rev.id).await.unwrap();
    tokio::time::sleep(Duration::from_millis(150)).await; // the fixture's two services take two slots
    let host = HttpHost::new(
        Arc::clone(&rt),
        HttpConfig {
            addr: ([127, 0, 0, 1], 0).into(),
            ..HttpConfig::default()
        },
    );
    let (tx, rx) = tokio::sync::oneshot::channel();
    let token = CancellationToken::new();
    let t = token.clone();
    tokio::spawn(async move {
        serve(host, t, |addr| {
            let _ = tx.send(addr);
        })
        .await
        .unwrap()
    });
    let addr = rx.await.unwrap();
    let client = reqwest::Client::new();
    // Two slow requests fill the remaining budget; the third is refused at once.
    let mut slow = Vec::new();
    for _ in 0..2 {
        let client = client.clone();
        slow.push(tokio::spawn(async move {
            client
                .get(format!("http://{addr}/slow"))
                .send()
                .await
                .unwrap()
                .status()
                .as_u16()
        }));
    }
    tokio::time::sleep(Duration::from_millis(200)).await;
    let t = std::time::Instant::now();
    let r = client
        .get(format!("http://{addr}/counter"))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 503);
    assert!(
        t.elapsed() < Duration::from_millis(500),
        "refusal must not wait"
    );
    let body: Value = r.json().await.unwrap();
    assert_eq!(body["error"]["code"], "capacity_exhausted");
    for s in slow {
        assert_eq!(s.await.unwrap(), 504);
    }
    tokio::time::sleep(Duration::from_millis(100)).await;
    let r = client
        .get(format!("http://{addr}/counter"))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200, "capacity recovered");
    token.cancel();
    rt.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn database_backend_loss_is_quarantined_and_recovered() {
    let Some(root) = fixture_root("pg-app") else {
        return;
    };
    let Some(server) = support::database_url() else {
        return;
    };
    let url = support::fresh_database(&server).await;
    let engine = usai_runtime::engine::from_env(64).unwrap();
    let out = build(
        engine.as_ref(),
        &BuildOptions {
            out_dir: out_dir("pg"),
            ..BuildOptions::for_project(&root)
        },
    )
    .await
    .unwrap();
    let u = url.clone();
    let rt = Runtime::with_env(
        engine,
        RuntimeConfig {
            cron_scheduler: false,
            queue_consumers: false,
            ..RuntimeConfig::default()
        },
        move |n| (n == "DATABASE_URL").then(|| u.clone()),
    );
    let rev = rt.install(out.definition).await.unwrap();
    rt.activate(rev.id).await.unwrap();
    rt.run_command("setup", vec![]).await.unwrap();
    let manager = rev.resources().get("main").cloned().unwrap();
    // Start a long query, then kill its backend from another connection.
    let m = Arc::clone(&manager);
    let victim = tokio::spawn(async move {
        m.call(usai_runtime::resource::ResourceCall { method: "one".into(), args: json!({ "sql": "select pg_sleep(30), pg_backend_pid() as pid", "params": [] }) }, CancellationToken::new()).await
    });
    tokio::time::sleep(Duration::from_millis(300)).await;
    let (admin, conn) = tokio_postgres::connect(&url, tokio_postgres::NoTls)
        .await
        .unwrap();
    tokio::spawn(conn);
    let killed = admin.execute("select pg_terminate_backend(pid) from pg_stat_activity where query like 'select pg_sleep(30)%' and pid <> pg_backend_pid()", &[]).await.unwrap();
    assert!(killed >= 1, "no backend killed");
    let result = victim.await.unwrap();
    let err = result.unwrap_err();
    assert!(
        matches!(err, usai_runtime::resource::ResourceError::Operation { .. }),
        "{err}"
    );
    tokio::time::sleep(Duration::from_millis(200)).await;
    let status = rt
        .status()
        .resources
        .into_iter()
        .find(|r| r.identity.kind == "postgres")
        .unwrap();
    assert_eq!(
        status.quarantined, 1,
        "a killed backend leaves no reusable-looking connection: {status:?}"
    );
    assert_eq!(status.in_use, 0);
    // Recovery on a replacement connection.
    let r = rt.invoke("http:GET /users/:id", json!({ "kind": "http", "request": { "method": "GET", "path": "/users/1", "url": "/users/1", "params": { "id": "1" }, "query": {}, "headers": {}, "body": null } })).await.unwrap();
    assert_eq!(r.outcome.unwrap().unwrap()["status"], 200);
    rt.shutdown().await;
}
