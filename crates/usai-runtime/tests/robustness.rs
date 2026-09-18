//! Security qualification, first pass: the surfaces that take untrusted
//! bytes never panic the host and never execute what they were not meant
//! to. Deterministic pseudo-random mutation (a fixed-seed xorshift), so a
//! failure reproduces from the printed seed. Not a substitute for a
//! coverage-guided fuzzer; a floor.
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;
use usai_runtime::build::{BuildOptions, build, load_artifact};
use usai_runtime::control::{ControlConfig, ControlHost, serve};
use usai_runtime::*;

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        // xorshift64*
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
    fn byte(&mut self) -> u8 {
        (self.next() & 0xff) as u8
    }
}

/// Mutates JSON structurally: drops keys, retypes values, swaps in huge
/// numbers, deep nesting, empty strings, NUL bytes.
fn mutate_json(rng: &mut Rng, value: &Value, depth: usize) -> Value {
    if depth > 6 {
        return Value::Null;
    }
    match rng.below(12) {
        0 => Value::Null,
        1 => json!(i64::MAX),
        2 => json!(-1),
        3 => json!(""),
        4 => json!("\u{0}\u{ffff}𝕏"),
        5 => json!("x".repeat(rng.below(5000))),
        6 => Value::Array(vec![Value::Null; 64]),
        7 => {
            let mut nested = json!({});
            for _ in 0..rng.below(64) {
                nested = json!({ "a": nested });
            }
            nested
        }
        _ => match value {
            Value::Object(map) => {
                let mut out = serde_json::Map::new();
                for (k, v) in map {
                    match rng.below(5) {
                        0 => {} // drop the key
                        1 => {
                            out.insert(format!("{k}\u{0}"), v.clone());
                        }
                        _ => {
                            out.insert(k.clone(), mutate_json(rng, v, depth + 1));
                        }
                    }
                }
                Value::Object(out)
            }
            Value::Array(items) => {
                let mut out = Vec::new();
                for v in items {
                    if rng.below(4) != 0 {
                        out.push(mutate_json(rng, v, depth + 1));
                    }
                }
                Value::Array(out)
            }
            Value::String(s) => match rng.below(3) {
                0 => json!(s.repeat(rng.below(50) + 1)),
                1 => json!(rng.below(1000)),
                _ => Value::String(s.clone()),
            },
            Value::Number(n) => match rng.below(3) {
                0 => json!(n.as_f64().unwrap_or(0.0) * 1e300),
                1 => json!(n.to_string()),
                _ => Value::Number(n.clone()),
            },
            other => other.clone(),
        },
    }
}

fn mutate_bytes(rng: &mut Rng, bytes: &[u8]) -> Vec<u8> {
    let mut out = bytes.to_vec();
    for _ in 0..(rng.below(16) + 1) {
        if out.is_empty() {
            out.push(rng.byte());
            continue;
        }
        let i = rng.below(out.len());
        match rng.below(4) {
            0 => out[i] = rng.byte(),
            1 => {
                out.remove(i);
            }
            2 => out.insert(i, rng.byte()),
            _ => {
                let end = (i + rng.below(64)).min(out.len());
                out.drain(i..end);
            }
        }
    }
    out
}

async fn hello_artifact() -> Option<PathBuf> {
    if std::process::Command::new("node")
        .arg("--version")
        .output()
        .is_err()
    {
        return None;
    }
    let hello = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/hello");
    if !hello.join("node_modules/@sakaladev/usai").exists() {
        return None;
    }
    let engine = usai_runtime::engine::from_env(8).ok()?;
    let out_dir = std::env::temp_dir().join(format!("usai-robust-{}", std::process::id()));
    build(
        engine.as_ref(),
        &BuildOptions {
            out_dir: out_dir.clone(),
            ..BuildOptions::for_project(&hello)
        },
    )
    .await
    .ok()?;
    Some(out_dir)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn mutated_manifests_and_bundles_are_refused_never_panic() {
    let Some(artifact) = hello_artifact().await else {
        eprintln!("skipping: fixture unavailable");
        return;
    };
    let manifest: Value =
        serde_json::from_slice(&std::fs::read(artifact.join("manifest.json")).unwrap()).unwrap();
    let code = std::fs::read(artifact.join("app.js")).unwrap();
    let seed = 0x9E37_79B9_7F4A_7C15u64;
    let mut rng = Rng(seed);
    let scratch = std::env::temp_dir().join(format!("usai-robust-mut-{}", std::process::id()));
    let mut loaded_ok = 0;
    for i in 0..400 {
        let _ = std::fs::remove_dir_all(&scratch);
        std::fs::create_dir_all(&scratch).unwrap();
        let mutated = mutate_json(&mut rng, &manifest, 0);
        std::fs::write(scratch.join("manifest.json"), mutated.to_string()).unwrap();
        let bundle = if i % 3 == 0 {
            mutate_bytes(&mut rng, &code)
        } else {
            code.clone()
        };
        std::fs::write(scratch.join("app.js"), &bundle).unwrap();
        // Loading is parsing + definition construction: must return, never unwind.
        let result = tokio::time::timeout(Duration::from_secs(10), load_artifact(&scratch)).await;
        match result {
            Ok(Ok(definition)) => {
                loaded_ok += 1;
                // A definition that loaded must also install without panicking
                // (compile of a mutated bundle is an error, not a crash).
                let engine = usai_runtime::engine::from_env(8).unwrap();
                let rt = Runtime::with_env(
                    engine,
                    RuntimeConfig {
                        cron_scheduler: false,
                        queue_consumers: false,
                        ..RuntimeConfig::default()
                    },
                    |_| None,
                );
                let _ = tokio::time::timeout(Duration::from_secs(30), rt.install(definition)).await;
                rt.shutdown().await;
            }
            Ok(Err(_)) => {}
            Err(_) => panic!("iteration {i} (seed {seed:#x}) hung loading a mutated artifact"),
        }
    }
    eprintln!("400 mutated artifacts: {loaded_ok} parsed into a definition, none panicked");
    // Raw garbage too.
    for _ in 0..200 {
        let _ = std::fs::remove_dir_all(&scratch);
        std::fs::create_dir_all(&scratch).unwrap();
        let mut garbage = vec![0u8; rng.below(4096)];
        for b in &mut garbage {
            *b = rng.byte();
        }
        std::fs::write(scratch.join("manifest.json"), &garbage).unwrap();
        std::fs::write(scratch.join("app.js"), &garbage).unwrap();
        assert!(load_artifact(&scratch).await.is_err());
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn control_and_http_surfaces_survive_garbage() {
    let Some(artifact) = hello_artifact().await else {
        return;
    };
    let engine = usai_runtime::engine::from_env(16).unwrap();
    let definition = load_artifact(&artifact).await.unwrap();
    let runtime = Runtime::with_env(
        engine,
        RuntimeConfig {
            cron_scheduler: false,
            queue_consumers: false,
            ..RuntimeConfig::default()
        },
        |_| None,
    );
    let rev = runtime.install(definition).await.unwrap();
    runtime.activate(rev.id).await.unwrap();
    let control = ControlHost::new(
        Arc::clone(&runtime),
        ControlConfig {
            addr: ([127, 0, 0, 1], 0).into(),
            token: Some("t".into()),
        },
    )
    .unwrap();
    let shutdown = CancellationToken::new();
    let (ctx, crx) = tokio::sync::oneshot::channel();
    let token = shutdown.clone();
    tokio::spawn(async move {
        serve(control, token, |a| {
            let _ = ctx.send(a);
        })
        .await
        .unwrap()
    });
    let control_addr = crx.await.unwrap();
    let http = usai_runtime::http::HttpHost::new(
        Arc::clone(&runtime),
        usai_runtime::http::HttpConfig {
            addr: ([127, 0, 0, 1], 0).into(),
            serve_status: true,
            ..Default::default()
        },
    );
    let (htx, hrx) = tokio::sync::oneshot::channel();
    let token = shutdown.clone();
    tokio::spawn(async move {
        usai_runtime::http::serve(http, token, |a| {
            let _ = htx.send(a);
        })
        .await
        .unwrap()
    });
    let http_addr = hrx.await.unwrap();

    let mut rng = Rng(0xDEAD_BEEF_CAFE_F00D);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap();
    let paths = [
        "/revisions",
        "/revisions/rev1/activate",
        "/revisions/rev1/drain",
        "/revisions/zzz",
        "/stop",
        "/invoke",
        "/health",
        "/status",
        "/../../etc/passwd",
        "/revisions/%00",
    ];
    for i in 0..300 {
        let path = paths[rng.below(paths.len())];
        let body = if i % 2 == 0 {
            mutate_json(&mut rng, &json!({ "artifact": artifact.to_string_lossy(), "workload": "http:GET /hello/:name", "input": { "kind": "http" } }), 0).to_string().into_bytes()
        } else {
            mutate_bytes(&mut rng, br#"{"artifact":"/nonexistent"}"#)
        };
        let method = ["GET", "POST", "DELETE", "PUT", "PATCH"][rng.below(5)];
        // Garbage with a valid token must not be able to do more than a
        // valid request could; draining or stopping the runtime is exactly
        // what a valid request may do, so those two carry a wrong token here
        // (the lifecycle test covers them legitimately).
        let destructive = path.ends_with("/drain") || path == "/stop";
        let req = client
            .request(
                method.parse().unwrap(),
                format!("http://{control_addr}{path}"),
            )
            .header(
                "authorization",
                if i % 7 == 0 || destructive {
                    "Bearer wrong"
                } else {
                    "Bearer t"
                },
            )
            .body(body);
        // Any answer is fine; a dropped connection (server panic) is not.
        let response = req.send().await.unwrap_or_else(|e| {
            panic!("control surface dropped the connection on {method} {path}: {e}")
        });
        assert!(response.status().as_u16() < 600);
    }
    // The application boundary: garbage paths, bodies, headers, methods.
    for _ in 0..300 {
        let raw_path = format!(
            "/hello/{}",
            String::from_utf8_lossy(&mutate_bytes(&mut rng, b"world"))
        );
        let path: String = raw_path
            .chars()
            .filter(|c| !c.is_control() && *c != ' ' && *c != '#' && *c != '?')
            .collect();
        let path = if path.starts_with('/') {
            path
        } else {
            format!("/{path}")
        };
        let method = ["GET", "POST", "OPTIONS", "TRACE", "DELETE"][rng.below(5)];
        let mut req = client
            .request(method.parse().unwrap(), format!("http://{http_addr}{path}"))
            .body(mutate_bytes(&mut rng, br#"{"a":1}"#));
        if rng.below(2) == 0 {
            req = req.header(
                "content-type",
                String::from_utf8_lossy(&mutate_bytes(&mut rng, b"application/json"))
                    .replace(['\r', '\n'], ""),
            );
        }
        if let Ok(response) = req.send().await {
            assert!(response.status().as_u16() < 600);
        }
    }
    // After all that, the runtime still serves and holds no leaked work.
    let ok = client
        .get(format!("http://{http_addr}/hello/x"))
        .send()
        .await
        .unwrap();
    assert_eq!(ok.status(), 200);
    shutdown.cancel();
    runtime.shutdown().await;
    let g = runtime.ledger().gauges.snapshot();
    assert_eq!(g.live_worlds, 0);
    assert_eq!(g.live_ops, 0);
}
