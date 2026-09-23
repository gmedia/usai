//! D2 acceptance: the HTTP contract workload end-to-end through a real
//! server, built from the real SDK via the real build pipeline.
//!
//! Requires `node` and an installed workspace (`pnpm install`); skips
//! otherwise so `cargo test` stays runnable on a bare machine.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;
use usai_runtime::build::{BuildOptions, build};
use usai_runtime::http::{HttpConfig, HttpHost, serve};
use usai_runtime::*;

struct Server {
    base: String,
    runtime: Arc<Runtime>,
    shutdown: CancellationToken,
    client: reqwest::Client,
}

async fn start() -> Option<Server> {
    if std::process::Command::new("node")
        .arg("--version")
        .output()
        .map(|o| !o.status.success())
        .unwrap_or(true)
    {
        eprintln!("skipping: node not available");
        return None;
    }
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/http-app");
    if !root.join("node_modules/@sakaladev/usai").exists() {
        eprintln!("skipping: fixture not installed (run `pnpm install`)");
        return None;
    }
    let engine = usai_runtime::engine::from_env(64).unwrap();
    // Tests run concurrently; each builds into its own directory.
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let out_dir = std::env::temp_dir().join(format!(
        "usai-http-test-{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    ));
    let options = BuildOptions {
        out_dir,
        ..BuildOptions::for_project(&root)
    };
    let out = build(engine.as_ref(), &options)
        .await
        .expect("fixture builds");
    let runtime = Runtime::with_env(
        engine,
        RuntimeConfig {
            default_timeout: Duration::from_secs(5),
            // The fixture declares crons for the workload tests; ticking
            // them here would leave live worlds behind the baseline checks.
            cron_scheduler: false,
            ..RuntimeConfig::default()
        },
        |name| match name {
            "GREETING" => Some("hi".to_owned()),
            "UPSTREAM_URL" => Some(upstream().to_owned()),
            _ => None,
        },
    );
    let rev = runtime.install(out.definition).await.unwrap();
    runtime.activate(rev.id).await.unwrap();
    let host = HttpHost::new(
        Arc::clone(&runtime),
        HttpConfig {
            addr: ([127, 0, 0, 1], 0).into(),
            expose_diagnostics: false,
            // The decode test posts 3 MB (the default bound is 1 MiB).
            max_body_bytes: 8 * 1024 * 1024,
            ..HttpConfig::default()
        },
    );
    let shutdown = CancellationToken::new();
    let (tx, rx) = tokio::sync::oneshot::channel();
    let token = shutdown.clone();
    tokio::spawn(async move {
        serve(host, token, |addr| {
            let _ = tx.send(addr);
        })
        .await
        .unwrap();
    });
    let addr = rx.await.unwrap();
    Some(Server {
        base: format!("http://{addr}"),
        runtime,
        shutdown,
        client: reqwest::Client::new(),
    })
}

/// A minimal HTTP/1.1 upstream the fixture's `http.client` talks to: echoes
/// method, path, headers and body as JSON; `/slow` never answers in time;
/// `/bytes` returns a non-UTF-8 body. One per test process.
fn upstream() -> &'static str {
    static UPSTREAM: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    UPSTREAM.get_or_init(|| {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                std::thread::spawn(move || serve_upstream(stream));
            }
        });
        format!("http://{addr}/")
    })
}

fn serve_upstream(mut stream: std::net::TcpStream) {
    use std::io::{Read, Write};
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    let (head_end, head) = loop {
        let n = match stream.read(&mut chunk) {
            Ok(0) | Err(_) => return,
            Ok(n) => n,
        };
        buf.extend_from_slice(&chunk[..n]);
        if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break (pos + 4, String::from_utf8_lossy(&buf[..pos]).to_string());
        }
    };
    let mut lines = head.lines();
    let request_line = lines.next().unwrap_or_default().to_owned();
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("").to_owned();
    let path = parts.next().unwrap_or("").to_owned();
    let mut headers = serde_json::Map::new();
    let mut length = 0usize;
    for line in lines {
        if let Some((k, v)) = line.split_once(':') {
            let k = k.trim().to_ascii_lowercase();
            let v = v.trim().to_owned();
            if k == "content-length" {
                length = v.parse().unwrap_or(0);
            }
            headers.insert(k, Value::String(v));
        }
    }
    while buf.len() < head_end + length {
        let n = match stream.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        buf.extend_from_slice(&chunk[..n]);
    }
    let body =
        String::from_utf8_lossy(&buf[head_end..(head_end + length).min(buf.len())]).to_string();
    let (status, content_type, payload): (u16, &str, Vec<u8>) = match path.as_str() {
        "/slow" => {
            std::thread::sleep(Duration::from_secs(3));
            (200, "text/plain", b"late".to_vec())
        }
        "/bytes" => (
            200,
            "application/octet-stream",
            vec![0xff, 0xfe, 0x00, 0x01],
        ),
        "/teapot" => (418, "text/plain", b"short and stout".to_vec()),
        _ => (
            200,
            "application/json",
            json!({ "method": method, "path": path, "headers": headers, "body": body })
                .to_string()
                .into_bytes(),
        ),
    };
    let _ = write!(
        stream,
        "HTTP/1.1 {status} X\r\ncontent-type: {content_type}\r\nx-echo: yes\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
        payload.len()
    );
    let _ = stream.write_all(&payload);
}

impl Server {
    async fn get(&self, path: &str) -> (u16, Value) {
        let r = self
            .client
            .get(format!("{}{path}", self.base))
            .send()
            .await
            .unwrap();
        let status = r.status().as_u16();
        let text = r.text().await.unwrap();
        (
            status,
            serde_json::from_str(&text).unwrap_or(Value::String(text)),
        )
    }

    async fn post_json(&self, path: &str, body: Value) -> (u16, Value) {
        let r = self
            .client
            .post(format!("{}{path}", self.base))
            .json(&body)
            .send()
            .await
            .unwrap();
        let status = r.status().as_u16();
        let text = r.text().await.unwrap();
        (
            status,
            serde_json::from_str(&text).unwrap_or(Value::String(text)),
        )
    }

    /// Ownership returns to baseline once the runtime has drained: a
    /// running service is a live world by design until then.
    async fn baseline(&self) {
        self.runtime.shutdown().await;
        tokio::time::sleep(Duration::from_millis(50)).await;
        let g = self.runtime.ledger().gauges.snapshot();
        assert_eq!(g.live_worlds, 0, "{g:?}");
        assert_eq!(g.live_ops, 0, "{g:?}");
        assert_eq!(self.runtime.status().worlds_in_use, 0);
    }
}

const UUID: &str = "6f1a2b3c-4d5e-4f60-8a71-92b3c4d5e6f7";

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn contract_endpoint_end_to_end() {
    let Some(s) = start().await else { return };
    let (status, body) = s.get(&format!("/users/{UUID}?page=3")).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(
        body,
        json!({ "id": UUID, "name": "user page 3", "email": "a@b.co" })
    );
    let (status, body) = s.get(&format!("/users/{UUID}")).await;
    assert_eq!(status, 200);
    assert_eq!(
        body["name"], "user page 1",
        "query default applied in-world"
    );
    s.baseline().await;
    s.shutdown.cancel();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn invalid_boundary_input_fails_before_any_world_exists() {
    let Some(s) = start().await else { return };
    // The fixture's services start asynchronously (one restarts twice by
    // design); wait until every service has settled before counting worlds.
    let started = std::time::Instant::now();
    loop {
        let services = s.runtime.active().unwrap().services();
        let settled = services
            .iter()
            .all(|x| x.state == usai_runtime::workloads::services::ServiceState::Running)
            && services
                .iter()
                .any(|x| x.name == "crashy" && x.restarts == 2);
        if settled {
            break;
        }
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "services did not settle: {services:?}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    // A service is reported running slightly before its world is counted;
    // wait until the gauge has been still for a moment before reading it.
    let mut before = s.runtime.ledger().gauges.snapshot();
    loop {
        tokio::time::sleep(Duration::from_millis(150)).await;
        let now = s.runtime.ledger().gauges.snapshot();
        if now.worlds_created == before.worlds_created {
            break;
        }
        before = now;
    }
    let (status, body) = s.get("/users/not-a-uuid").await;
    assert_eq!(status, 400, "{body}");
    assert_eq!(body["error"]["code"], "validation_failed");
    assert_eq!(body["error"]["details"]["slot"], "params");
    let (status, body) = s.get(&format!("/users/{UUID}?page=0")).await;
    assert_eq!(status, 400, "{body}");
    assert_eq!(body["error"]["details"]["slot"], "query");
    let (status, _) = s.get("/nowhere").await;
    assert_eq!(status, 404);
    let r = s
        .client
        .post(format!("{}/users", s.base))
        .header("content-type", "application/json")
        .body("{not json")
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 400);
    let r = s
        .client
        .post(format!("{}/users", s.base))
        .json(&json!({ "name": "", "email": "x" }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 400);
    // A missing required property points at the property (`/name`), as the
    // world's validator would — a form attaches the issue to its field.
    let r = s
        .client
        .post(format!("{}/users", s.base))
        .json(&json!({ "email": "a@b.co" }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 400);
    let body: Value = r.json().await.unwrap();
    let paths: Vec<&str> = body["error"]["details"]["issues"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|i| i["path"].as_str())
        .collect();
    assert!(paths.contains(&"/name"), "{body}");
    let r = s
        .client
        .put(format!("{}/users", s.base))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 405);
    let after = s.runtime.ledger().gauges.snapshot();
    assert_eq!(
        after.worlds_created, before.worlds_created,
        "a rejected request created a world"
    );
    s.shutdown.cancel();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn boundary_final_slots_keep_zod_semantics_without_a_second_parse() {
    let Some(s) = start().await else { return };
    // Skipped parse: undeclared keys are still stripped, declared kept.
    let r = s
        .client
        .post(format!("{}/shape", s.base))
        .json(&json!({ "a": "x", "n": 2, "undeclared": true }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    assert_eq!(
        r.json::<Value>().await.unwrap(),
        json!({ "keys": ["a", "n"] })
    );
    // Absent optional stays absent.
    let r = s
        .client
        .post(format!("{}/shape", s.base))
        .json(&json!({ "a": "x" }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.json::<Value>().await.unwrap(), json!({ "keys": ["a"] }));
    // A strict object refuses the unknown key before any world exists.
    let r = s
        .client
        .post(format!("{}/shape-strict", s.base))
        .json(&json!({ "a": "x", "extra": 1 }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 400);
    let body = r.json::<Value>().await.unwrap();
    assert_eq!(body["error"]["code"], "validation_failed", "{body}");
    assert!(
        body["error"]["details"]["issues"]
            .to_string()
            .contains("extra"),
        "{body}"
    );
    let r = s
        .client
        .post(format!("{}/shape-strict", s.base))
        .json(&json!({ "a": "x" }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.json::<Value>().await.unwrap(), json!({ "keys": ["a"] }));
    // A transform is not final: the world parses and the handler sees its output.
    let r = s
        .client
        .post(format!("{}/shape-transform", s.base))
        .json(&json!({ "a": "x" }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    assert_eq!(r.json::<Value>().await.unwrap(), json!({ "a": "X" }));
    // The boundary still rejects before any world exists.
    let r = s
        .client
        .post(format!("{}/shape", s.base))
        .json(&json!({ "a": 1 }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 400);
    s.baseline().await;
    s.shutdown.cancel();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn created_and_no_content_helpers() {
    let Some(s) = start().await else { return };
    let r = s
        .client
        .post(format!("{}/users", s.base))
        .json(&json!({ "name": "Ayu", "email": "ayu@x.io" }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 201);
    let body: Value = r.json().await.unwrap();
    assert_eq!(body["name"], "Ayu");
    assert_eq!(body["id"], UUID);
    let r = s
        .client
        .delete(format!("{}/users/{UUID}", s.base))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 204);
    assert_eq!(r.text().await.unwrap(), "");
    s.shutdown.cancel();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fresh_world_per_request_and_persistent_resource() {
    let Some(s) = start().await else { return };
    for _ in 0..3 {
        let (status, body) = s.get("/counter").await;
        assert_eq!(status, 200);
        assert_eq!(body["counter"], 1, "mutable globals leaked across requests");
    }
    for expected in 1..=3 {
        let r = s
            .client
            .post(format!("{}/hits", s.base))
            .send()
            .await
            .unwrap();
        let body: Value = r.json().await.unwrap();
        assert_eq!(body["hits"], expected);
    }
    s.baseline().await;
    s.shutdown.cancel();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_requests_do_not_share_state() {
    let Some(s) = start().await else { return };
    let mut handles = Vec::new();
    for _ in 0..24 {
        let client = s.client.clone();
        let url = format!("{}/counter", s.base);
        handles.push(tokio::spawn(async move {
            let body: Value = client.get(url).send().await.unwrap().json().await.unwrap();
            body["counter"].as_u64().unwrap()
        }));
    }
    for h in handles {
        assert_eq!(h.await.unwrap(), 1);
    }
    s.baseline().await;
    s.shutdown.cancel();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn auth_boundary() {
    let Some(s) = start().await else { return };
    let (status, body) = s.get("/me").await;
    assert_eq!(status, 401, "{body}");
    assert_eq!(body["error"]["code"], "unauthorized");
    let r = s
        .client
        .get(format!("{}/me", s.base))
        .bearer_auth("wrong")
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 401);
    let r = s
        .client
        .get(format!("{}/me", s.base))
        .bearer_auth("secret")
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    assert_eq!(r.json::<Value>().await.unwrap(), json!({ "userId": "u1" }));
    s.shutdown.cancel();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn errors_are_contracts_and_unexpected_failures_are_sanitized() {
    let Some(s) = start().await else { return };
    let (status, body) = s.get("/users/00000000-0000-0000-0000-000000000000").await;
    assert_eq!(status, 404);
    assert_eq!(body["error"]["code"], "not_found");
    assert_eq!(body["error"]["message"], "user not found");
    assert_eq!(
        body["error"]["details"]["id"],
        "00000000-0000-0000-0000-000000000000"
    );
    let (status, body) = s.get("/boom").await;
    assert_eq!(status, 500);
    assert_eq!(body["error"]["code"], "internal");
    assert!(
        !body.to_string().contains("secret detail"),
        "internal detail leaked: {body}"
    );
    // An operation's failure code (a SQLSTATE) is not a contract the client
    // can act on: `internal` outside diagnostics, the code in the log.
    let (status, body) = s.get("/boom-operation").await;
    assert_eq!(status, 500);
    assert_eq!(body["error"]["code"], "internal", "{body}");
    assert!(!body.to_string().contains("22003"), "{body}");
    let (status, body) = s.get("/bad-shape").await;
    assert_eq!(status, 500, "{body}");
    assert_eq!(body["error"]["code"], "response_contract_violation");
    // The stack the developer sees (dev responses, logs) points at the
    // TypeScript source, not at a line in the 800 KB bundle.
    let r = s
        .runtime
        .invoke(
            "http:GET /boom",
            json!({ "kind": "http", "env": {}, "request": { "method": "GET", "path": "/boom", "url": "/boom", "params": {}, "query": {}, "headers": {}, "body": null } }),
        )
        .await
        .unwrap();
    let Some(Err(error)) = r.outcome else {
        panic!("boom must fail: {:?}", r.outcome)
    };
    let stack = error.stack.expect("a stack");
    // The frame points at the `throw` in the fixture, whatever line the
    // formatter put it on.
    let source = std::fs::read_to_string(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/http-app/src/app.ts"),
    )
    .unwrap();
    let line = source
        .lines()
        .position(|l| l.contains("kaboom with secret detail"))
        .expect("the fixture throws")
        + 1;
    assert!(
        stack.contains(&format!("src/app.ts:{line}:")),
        "unmapped stack:\n{stack}"
    );
    assert!(
        !stack.contains("usai:app:"),
        "unmapped frame left:\n{stack}"
    );
    s.shutdown.cancel();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn detached_work_is_reported_and_the_response_still_commits() {
    let Some(s) = start().await else { return };
    let (status, body) = s.get("/detach").await;
    assert_eq!(
        status, 200,
        "a pending timer is a diagnostic, not a lost side effect"
    );
    assert_eq!(body["ok"], true);
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(
        s.runtime.ledger().gauges.snapshot().detached_work_detected,
        1
    );
    // A write left in flight is different: the runtime cancelled it, so a
    // 200 would report success over lost work. The request fails instead.
    let r = s
        .client
        .post(format!("{}/detach-write", s.base))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 500);
    let body: Value = r.json().await.unwrap();
    assert_eq!(body["error"]["code"], "detached_work");
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("cancelled"),
        "{body}"
    );
    s.baseline().await;
    s.shutdown.cancel();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn deadline_maps_to_gateway_timeout() {
    let Some(s) = start().await else { return };
    let (status, body) = s.get("/slow").await;
    assert_eq!(status, 504, "{body}");
    assert_eq!(body["error"]["code"], "deadline_exceeded");
    // Synchronous work that never awaits is bounded by the same deadline.
    let t = std::time::Instant::now();
    let (status, body) = s.get("/busy").await;
    assert_eq!(status, 504, "{body}");
    assert_eq!(body["error"]["code"], "deadline_exceeded");
    assert!(
        t.elapsed() < Duration::from_secs(2),
        "interrupted at the deadline, not at the end of the loop: {:?}",
        t.elapsed()
    );
    tokio::time::sleep(Duration::from_millis(50)).await;
    s.baseline().await;
    s.shutdown.cancel();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn client_disconnect_cancels_the_world() {
    let Some(s) = start().await else { return };
    let client = reqwest::Client::builder()
        .timeout(Duration::from_millis(150))
        .build()
        .unwrap();
    let err = client
        .get(format!("{}/slow", s.base))
        .send()
        .await
        .unwrap_err();
    assert!(err.is_timeout());
    tokio::time::sleep(Duration::from_millis(200)).await;
    s.baseline().await;
    s.shutdown.cancel();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn raw_escape_hatch_sees_exact_bytes() {
    let Some(s) = start().await else { return };
    let r = s
        .client
        .post(format!("{}/webhook", s.base))
        .header("content-type", "application/xml")
        .body("<a>1</a>")
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    assert_eq!(r.headers().get("x-raw").unwrap(), "1");
    assert_eq!(r.text().await.unwrap(), "len=8;ct=application/xml");
    s.shutdown.cancel();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn query_and_headers_reach_the_handler() {
    let Some(s) = start().await else { return };
    let r = s
        .client
        .get(format!("{}/echo?a=1&a=2&b=x", s.base))
        .header("x-a", "yes")
        .send()
        .await
        .unwrap();
    let body: Value = r.json().await.unwrap();
    assert_eq!(body["query"], json!({ "a": ["1", "2"], "b": "x" }));
    assert_eq!(body["headers"]["x-a"], "yes");
    s.shutdown.cancel();
}

/// `ctx.resources["x"]` on a workload that did not declare `x` fails with
/// a named error that says what to add, not with `undefined`.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_undeclared_resource_is_a_named_error() {
    let Some(s) = start().await else { return };
    let (status, body) = s.get("/undeclared").await;
    assert_eq!(status, 500, "{body}");
    assert_eq!(body["error"]["code"], "resource_not_declared", "{body}");
    s.shutdown.cancel();
}

/// A world has the Web globals a handler reasonably expects — `URL`,
/// `URLSearchParams`, `structuredClone` — so schema checks that need them
/// (`z.string().url()`) work in-world and handlers can use them.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn worlds_have_url_and_structured_clone() {
    let Some(s) = start().await else { return };
    let r = s
        .client
        .post(format!("{}/url", s.base))
        .json(&json!({ "url": "https://News.ycombinator.com:443/item?id=1" }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    let body: Value = r.json().await.unwrap();
    assert_eq!(body["host"], "news.ycombinator.com");
    assert_eq!(body["path"], "/item");
    assert_eq!(
        body["href"],
        "https://news.ycombinator.com/item?id=1&seen=1"
    );
    assert_eq!(body["cloned"], true);
    let r = s
        .client
        .post(format!("{}/url", s.base))
        .json(&json!({ "url": "not a url" }))
        .send()
        .await
        .unwrap();
    assert_eq!(
        r.status(),
        400,
        "an invalid URL is refused, a valid one is not"
    );
    s.shutdown.cancel();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn outbound_http_is_a_declared_owned_resource() {
    let Some(s) = start().await else { return };
    // A request through the declared client: method, headers, body arrive;
    // the response is readable as JSON.
    let r = s
        .client
        .post(format!("{}/egress", s.base))
        .json(&json!({ "path": "/orders/1?x=2", "method": "PUT", "json": { "n": 1 } }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    let body: Value = r.json().await.unwrap();
    assert_eq!(body["status"], 200, "{body}");
    assert_eq!(body["ok"], true);
    assert_eq!(body["echo"], "yes");
    assert_eq!(body["body"]["method"], "PUT");
    assert_eq!(body["body"]["path"], "/orders/1?x=2");
    assert_eq!(body["body"]["body"], "{\"n\":1}");
    assert_eq!(body["body"]["headers"]["content-type"], "application/json");
    assert!(
        body["body"]["headers"]["user-agent"]
            .as_str()
            .unwrap()
            .starts_with("usai/"),
        "{body}"
    );
    // A non-2xx answer is data, not an exception.
    let r = s
        .client
        .post(format!("{}/egress", s.base))
        .json(&json!({ "path": "/teapot" }))
        .send()
        .await
        .unwrap();
    let body: Value = r.json().await.unwrap();
    assert_eq!(body["status"], 418);
    assert_eq!(body["ok"], false);
    assert_eq!(body["body"], "short and stout");
    // Binary bodies arrive as bytes.
    let (status, body) = s.get("/egress/bytes").await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["length"], 4);
    assert_eq!(body["first"], 255);
    // The declared baseUrl is the destination: another origin is refused
    // inside the world, with a code the handler can act on.
    let (_, body) = s.get("/egress/other").await;
    assert_eq!(body["code"], "origin_refused");
    // A world past its deadline drops its request: 504 to the caller, the
    // operation counted as cancelled, nothing left in flight.
    let (status, _) = s.get("/egress/slow").await;
    assert_eq!(status, 504);
    let resource = s
        .runtime
        .status()
        .resources
        .into_iter()
        .find(|r| r.identity.kind == "http.client")
        .expect("the client is a resource with a status");
    assert_eq!(resource.detail["cancelled"], 1, "{resource:?}");
    assert_eq!(resource.in_use, 0);
    assert_eq!(resource.detail["baseUrl"], upstream());
    // The global `fetch` exists only to say what to do instead.
    let (_, body) = s.get("/nofetch").await;
    assert_eq!(body["code"], "fetch_not_available");
    assert!(
        body["message"].as_str().unwrap().contains("httpClient"),
        "{body}"
    );
    s.baseline().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn worlds_have_crypto_and_password_hashing() {
    let Some(s) = start().await else { return };
    let (status, body) = s.get("/crypto").await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(
        body["sha256"],
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(body["hmac"], hmac_sha256(b"k", b"abc"));
    assert_eq!(body["verified"], true);
    let uuid = body["uuid"].as_str().unwrap();
    assert_eq!(uuid.len(), 36);
    assert_eq!(&uuid[14..15], "4", "version 4: {uuid}");
    assert_ne!(body["uuid"], body["other"], "two draws differ");
    // Entropy is per world: two worlds never draw the same bytes.
    let (_, again) = s.get("/crypto").await;
    assert_ne!(again["uuid"], body["uuid"]);
    assert_ne!(again["random"], body["random"]);
    let r = s
        .client
        .post(format!("{}/password", s.base))
        .json(&json!({ "password": "correct horse battery staple" }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    let body: Value = r.json().await.unwrap();
    assert_eq!(body["prefix"], "$argon2id$");
    assert_eq!(body["ok"], true);
    assert_eq!(body["wrong"], false);
    s.baseline().await;
}

fn hmac_sha256(key: &[u8], data: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut k = [0u8; 64];
    k[..key.len()].copy_from_slice(key);
    let ipad: Vec<u8> = k.iter().map(|b| b ^ 0x36).collect();
    let opad: Vec<u8> = k.iter().map(|b| b ^ 0x5c).collect();
    let inner = Sha256::digest([ipad.as_slice(), data].concat());
    hex::encode(Sha256::digest([opad.as_slice(), inner.as_slice()].concat()))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn manifest_describes_the_application() {
    let Some(s) = start().await else { return };
    let rev = s.runtime.active().unwrap();
    let m = rev.definition.manifest();
    assert_eq!(m.name, "http-fixture");
    assert_eq!(m.modules[0].name, "users");
    let get_user = rev.definition.workload("http:GET /users/:id").unwrap().1;
    assert_eq!(get_user.module.as_deref(), Some("users"));
    assert!(
        get_user.contracts.params.is_some(),
        "zod described params as JSON Schema"
    );
    assert!(get_user.contracts.response.contains_key(&200));
    // Validate once: params (a uuid string) and the query (a default and
    // an optional array) are final at the boundary.
    assert_eq!(get_user.contracts.boundary_final, ["params", "query"]);
    let create = rev.definition.workload("http:POST /users").unwrap().1;
    assert_eq!(create.contracts.boundary_final, ["body"]);
    let shaped = rev
        .definition
        .workload("http:POST /shape-transform")
        .unwrap()
        .1;
    assert!(shaped.contracts.boundary_final.is_empty());
    let me = rev.definition.workload("http:GET /me").unwrap().1;
    assert_eq!(me.auth.as_deref(), Some("token"));
    let webhook = rev.definition.workload("http:POST /webhook").unwrap().1;
    assert!(matches!(
        webhook.trigger,
        usai_runtime::definition::Trigger::Http { raw: true, .. }
    ));
    assert_eq!(m.resources[0].kind, "cache.local");
    assert_eq!(m.env[0].name, "GREETING");
    assert!(rev.definition.workload("task:send-receipt").is_some());
    s.shutdown.cancel();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn openapi_is_generated_from_the_definition() {
    let Some(s) = start().await else { return };
    let rev = s.runtime.active().unwrap();
    // The document carries the runtime's effective defaults (deadline), so
    // the served one is generated with this runtime's configuration.
    let doc = usai_runtime::openapi::generate_with(
        &rev.definition,
        s.runtime.config(),
        usai_runtime::openapi::Profile::Internal,
    );
    assert_eq!(doc["openapi"], "3.1.0");
    assert_eq!(doc["info"]["title"], "http-fixture");
    assert_eq!(
        doc["info"]["description"],
        "The HTTP test fixture: one of everything the pipeline can serve.",
        "defineApp({{ description }}) reaches the document"
    );
    assert_eq!(doc["info"]["version"], rev.definition.identity());
    let get_user = &doc["paths"]["/users/{id}"]["get"];
    assert_eq!(get_user["tags"], json!(["users"]));
    assert_eq!(get_user["summary"], "One user");
    assert_eq!(
        get_user["description"],
        "By id, with the page the caller asked for."
    );
    assert!(
        doc["paths"]["/users"]["post"].get("summary").is_none(),
        "no summary is invented"
    );
    assert_eq!(get_user["responses"]["200"]["description"], "OK");
    assert_eq!(
        doc["paths"]["/users"]["post"]["responses"]["201"]["description"],
        "Created"
    );
    assert!(get_user["responses"]["503"].is_object() && get_user["responses"]["504"].is_object());
    assert!(
        !doc["paths"].to_string().contains("9007199254740991"),
        "safe-integer bounds are not a contract"
    );
    let params: Vec<(String, String, bool)> = get_user["parameters"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| {
            (
                p["name"].as_str().unwrap().into(),
                p["in"].as_str().unwrap().into(),
                p["required"].as_bool().unwrap(),
            )
        })
        .collect();
    assert!(params.contains(&("id".into(), "path".into(), true)));
    assert!(params.contains(&("page".into(), "query".into(), false)));
    assert_eq!(
        get_user["responses"]["200"]["content"]["application/json"]["schema"]["type"],
        "object"
    );
    assert!(
        get_user["responses"]["400"].is_object(),
        "validated endpoints document 400"
    );
    assert_eq!(
        doc["paths"]["/users"]["post"]["requestBody"]["required"],
        true
    );
    assert_eq!(
        doc["paths"]["/users"]["post"]["responses"]["201"]["content"]["application/json"]["schema"]
            ["required"],
        json!(["id", "name", "email"])
    );
    assert_eq!(
        doc["paths"]["/me"]["get"]["security"],
        json!([{ "token": [] }])
    );
    assert_eq!(
        doc["components"]["securitySchemes"]["token"]["scheme"],
        "bearer"
    );
    // A custom scheme that declares its credential is an apiKey there; one
    // that declares nothing gets no invented header — the operation says so.
    assert_eq!(
        doc["components"]["securitySchemes"]["session"],
        json!({ "type": "apiKey", "in": "cookie", "name": "sid", "description": "the sid cookie set by /login" })
    );
    assert_eq!(
        doc["paths"]["/me/cookie"]["get"]["security"],
        json!([{ "session": [] }])
    );
    assert!(
        doc["paths"]["/me/cookie"]["get"]["x-usai-resources"]
            .as_array()
            .is_some_and(|r| r.iter().any(|x| x["name"] == "sessions")),
        "the scheme's resource is the operation's: {}",
        doc["paths"]["/me/cookie"]["get"]["x-usai-resources"]
    );
    assert!(doc["components"]["securitySchemes"].get("opaque").is_none());
    assert!(doc["paths"]["/me/opaque"]["get"].get("security").is_none());
    assert!(
        doc["paths"]["/me/opaque"]["get"]["description"]
            .as_str()
            .unwrap()
            .contains("custom scheme `opaque`"),
        "{}",
        doc["paths"]["/me/opaque"]["get"]
    );
    assert_eq!(doc["paths"]["/me/opaque"]["get"]["x-usai-auth"], "opaque");
    let webhook = &doc["paths"]["/webhook"]["post"];
    assert_eq!(
        webhook["x-usai-raw"], true,
        "raw endpoints are opaque, not invented"
    );
    // …but the statuses the handler declares (`responses`) do reach the document.
    assert_eq!(
        webhook["responses"]["401"]["description"], "bad signature",
        "{webhook}"
    );
    assert!(webhook["responses"].get("default").is_none(), "{webhook}");
    // A WebSocket is not a raw HTTP exchange: 101/426, no request body.
    let chat = &doc["paths"]["/chat"]["get"];
    assert_eq!(
        chat["x-usai-socket"]["incoming"]["properties"]["text"]["type"], "string",
        "{chat}"
    );
    assert_eq!(
        chat["x-usai-socket"]["outgoing"]["properties"]["count"]["type"], "number",
        "{chat}"
    );
    // A 405 names what is allowed; a body without a JSON media type on a
    // JSON contract is a 415, not "null is not an object".
    let r = s
        .client
        .patch(format!("{}/users", s.base))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 405);
    assert_eq!(r.headers().get("allow").unwrap(), "POST");
    let r = s
        .client
        .post(format!("{}/users", s.base))
        .header("content-type", "text/plain")
        .body("name=x")
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 415, "{}", r.text().await.unwrap());
    assert!(chat["responses"].get("101").is_some(), "{chat}");
    assert!(chat["responses"].get("426").is_some(), "{chat}");
    assert!(chat.get("x-usai-raw").is_none(), "{chat}");
    assert!(chat.get("requestBody").is_none(), "{chat}");
    // The facts a Usai consumer can rely on travel with the operation.
    let order = &doc["paths"]["/orders"]["post"];
    assert_eq!(order["x-usai-validated"]["body"], "before-world");
    // A stream's declared events are named components and listed on the
    // response, so a client knows what to listen for and what arrives.
    let events = &doc["paths"]["/events"]["get"];
    assert_eq!(
        events["x-usai-events"]["tick"]["$ref"], "#/components/schemas/EventsEventTick",
        "{events}"
    );
    assert_eq!(
        doc["components"]["schemas"]["EventsEventTick"]["properties"]["i"]["type"],
        "integer"
    );
    assert!(
        events["responses"]["200"]["description"]
            .as_str()
            .unwrap()
            .contains("events: done, tick"),
        "{events}"
    );
    // The application's operationId wins over the derived one; a socket's
    // message contracts are named components.
    assert_eq!(doc["paths"]["/users"]["post"]["operationId"], "createUser");
    assert_eq!(doc["paths"]["/chat"]["get"]["operationId"], "chat");
    assert_eq!(
        doc["components"]["schemas"]["ChatIncoming"]["properties"]["text"]["type"], "string",
        "{}",
        doc["components"]["schemas"]
    );
    assert!(doc["components"]["schemas"]["ChatOutgoing"]["properties"]["echo"].is_object());
    // Documented response headers appear on their status (and `"*"` on every
    // success status), typed as strings for a generated client.
    let created = &doc["paths"]["/users"]["post"]["responses"]["201"];
    assert_eq!(
        created["headers"]["location"]["description"], "URL of the new user",
        "{created}"
    );
    assert_eq!(created["headers"]["location"]["schema"]["type"], "string");
    assert_eq!(
        created["headers"]["etag"]["description"], "Version of the user",
        "{created}"
    );
    assert!(
        doc["paths"]["/users"]["post"]["responses"]["400"]
            .get("headers")
            .is_none(),
        "`*` covers the success statuses, not the errors"
    );
    // Declared errors are typed: the code is an enum a generated client can
    // switch on, and the description is the status's reason phrase.
    let not_found = &doc["paths"]["/users/{id}"]["get"]["responses"]["404"];
    assert_eq!(
        not_found["description"], "Not Found: code not_found",
        "{not_found}"
    );
    assert_eq!(
        not_found["content"]["application/json"]["schema"]["allOf"][1]["properties"]["error"]["properties"]
            ["code"]["enum"],
        json!(["not_found"]),
        "{not_found}"
    );
    // A raw GET has no request body and lists its path parameters.
    let image = &doc["paths"]["/images/{id}"]["get"];
    assert!(image.get("requestBody").is_none(), "{image}");
    assert_eq!(image["parameters"][0]["name"], "id", "{image}");
    assert_eq!(image["parameters"][0]["in"], "path");
    assert!(
        doc["paths"]["/decode"]["post"].get("requestBody").is_some(),
        "a raw POST still takes a body"
    );
    // A transforming schema is refused at the boundary and parsed again in
    // the world; the document says so instead of hiding it.
    assert_eq!(
        doc["paths"]["/shape-transform"]["post"]["x-usai-validated"]["body"],
        "both"
    );
    assert!(
        doc["paths"]["/counter"]["get"]["responses"]["400"].is_null()
            || !doc["paths"]["/counter"]["get"]["responses"]["400"]["description"]
                .as_str()
                .unwrap()
                .contains("invalid_json"),
        "a route without a body does not promise invalid_json"
    );
    assert!(
        doc["paths"]["/me"]["get"]["responses"]["401"]["description"]
            .as_str()
            .is_some_and(|d| d.contains("unauthorized")),
        "{}",
        doc["paths"]["/me"]["get"]["responses"]["401"]
    );
    assert_eq!(order["x-usai-resources"][0]["name"], "audit");
    assert_eq!(order["x-usai-dispatches"], json!(["task:record"]));
    assert_eq!(order["x-usai-lifetime"], "request");
    assert_eq!(order["x-usai-timeout-source"], "default");
    assert!(order["x-usai-timeout-ms"].as_u64().unwrap() > 0);
    assert!(
        doc["x-usai-workloads"]
            .as_array()
            .unwrap()
            .iter()
            .any(|w| w["id"] == "task:record" && w["kind"] == "task"),
        "non-HTTP workloads are part of the document"
    );
    assert!(
        doc["x-usai-resources"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["name"] == "audit")
    );
    assert!(webhook.get("parameters").is_none());
    assert!(
        doc.to_string().find("$schema").is_none_or(|_| false)
            || !doc["paths"].to_string().contains("\"$schema\"")
    );
    // The public profile is the consumer contract: same paths, parameters,
    // bodies, responses and security; no runtime facts, no inventory. The
    // declared error codes survive in the response descriptions, where a
    // standard reader looks.
    let public = usai_runtime::openapi::generate_with(
        &rev.definition,
        s.runtime.config(),
        usai_runtime::openapi::Profile::Public,
    );
    assert_eq!(
        public["paths"]["/users/{id}"]["get"]["parameters"],
        get_user["parameters"]
    );
    assert_eq!(
        public["paths"]["/users"]["post"]["requestBody"],
        doc["paths"]["/users"]["post"]["requestBody"]
    );
    assert_eq!(
        public["paths"]["/me"]["get"]["security"],
        json!([{ "token": [] }])
    );
    assert_eq!(
        public["components"]["securitySchemes"],
        doc["components"]["securitySchemes"]
    );
    assert_eq!(public["info"]["title"], "http-fixture");
    assert_eq!(
        public["paths"]["/users/{id}"]["get"]["responses"]["404"]["description"],
        "Not Found: code not_found"
    );
    // The public profile keeps the socket message components.
    assert!(
        public["components"]["schemas"]["ChatIncoming"].is_object(),
        "{}",
        public["components"]
    );
    let text = public.to_string();
    assert!(
        !text.contains("x-usai-"),
        "public profile leaks an extension: {text}"
    );
    for key in ["x-usai-workloads", "x-usai-resources", "x-usai-env"] {
        assert!(public.get(key).is_none(), "public profile carries {key}");
    }
    assert!(public["info"].get("x-usai-identity").is_none());
    // The dev server serves the same document.
    let host = HttpHost::new(
        Arc::clone(&s.runtime),
        HttpConfig {
            addr: ([127, 0, 0, 1], 0).into(),
            serve_docs: true,
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
    let served: Value = s
        .client
        .get(format!("http://{addr}/_usai/openapi.json"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(served, doc);
    let served_public: Value = s
        .client
        .get(format!("http://{addr}/_usai/openapi.json?profile=public"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(served_public, public);
    let unknown = s
        .client
        .get(format!("http://{addr}/_usai/openapi.json?profile=secret"))
        .send()
        .await
        .unwrap();
    assert_eq!(unknown.status(), 400);
    // A client that asks for JSON gets the OpenAPI document from the docs
    // URL; everyone else (browsers, curl) gets the page.
    let raw = s
        .client
        .get(format!("http://{addr}/_usai/docs"))
        .header("accept", "application/json")
        .send()
        .await
        .unwrap();
    assert_eq!(
        raw.headers().get("content-type").unwrap(),
        "application/json"
    );
    let as_json: Value = raw.json().await.unwrap();
    assert_eq!(as_json, doc);
    let docs = s
        .client
        .get(format!("http://{addr}/_usai/docs"))
        .send()
        .await
        .unwrap();
    assert_eq!(
        docs.headers().get("content-type").unwrap(),
        "text/html; charset=utf-8"
    );
    let html = docs.text().await.unwrap();
    for needle in [
        "openapi.json",
        "x-usai-validated",
        "x-usai-dispatches",
        "Try it",
        "prefers-color-scheme",
        "prefers-reduced-motion",
        "Skip to content",
    ] {
        assert!(html.contains(needle), "docs page lacks {needle}");
    }
    assert!(
        !html.contains("<script src")
            && !html.contains("<link rel=\"stylesheet\"")
            && !html.contains("@import"),
        "the docs page loads nothing from the network"
    );
    // Not served on a production host.
    let (status, _) = s.get("/_usai/openapi.json").await;
    assert_eq!(status, 404);
    token.cancel();
    s.shutdown.cancel();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn status_and_metrics_derive_from_runtime_truth() {
    let Some(s) = start().await else { return };
    let host = HttpHost::new(
        Arc::clone(&s.runtime),
        HttpConfig {
            addr: ([127, 0, 0, 1], 0).into(),
            serve_status: true,
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
    let base = format!("http://{addr}");
    let _ = s
        .client
        .get(format!("{base}/counter"))
        .send()
        .await
        .unwrap();
    let _ = s
        .client
        .get(format!("{base}/users/not-a-uuid"))
        .send()
        .await
        .unwrap();
    let status: Value = s
        .client
        .get(format!("{base}/_usai/status"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(status["engine"] == "quickjs" || status["engine"] == "wasm");
    assert_eq!(status["http"]["responses_2xx"], 1);
    assert_eq!(status["http"]["responses_4xx"], 1);
    assert_eq!(status["http"]["rejected_before_world"], 1);
    assert_eq!(status["revisions"][0]["state"], "active");
    assert!(status["gauges"]["worldsCreated"].as_u64().unwrap() >= 1);
    let metrics = s
        .client
        .get(format!("{base}/_usai/metrics"))
        .send()
        .await
        .unwrap();
    assert!(
        metrics
            .headers()
            .get("content-type")
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("text/plain")
    );
    let text = metrics.text().await.unwrap();
    assert!(
        text.contains("usai_http_rejected_before_world_total 1"),
        "{text}"
    );
    assert!(text.contains("usai_http_responses_total{class=\"2xx\"} 1"));
    assert!(text.contains("usai_resource{kind=\"cache.local\",name=\"hits\",metric=\"max\"}"));
    assert!(text.contains("usai_resource{kind=\"cache.local\",name=\"hits\",metric=\"ready\"} 1"));
    assert!(
        text.contains("# TYPE usai_resource_quarantines_total counter")
            && text.contains("usai_resource_quarantines_total{kind=\"cache.local\",name=\"hits\"}"),
        "quarantines are a counter, not a level: {text}"
    );
    assert!(!text.contains("metric=\"quarantined\""));
    // Round two: why requests were refused, and how long they took.
    assert!(
        text.contains("usai_http_rejections_total{reason=\"validation\"} 1"),
        "{text}"
    );
    assert!(text.contains("usai_http_rejections_total{reason=\"capacity\"} 0"));
    // The histogram is admitted requests only: the validation refusal was
    // decided before a world existed and must not improve the p99.
    assert!(text.contains("# TYPE usai_http_request_seconds histogram"));
    assert!(
        text.contains("usai_http_request_seconds_bucket{le=\"+Inf\"} 1"),
        "{text}"
    );
    assert!(text.contains("usai_http_request_seconds_count 1"));
    assert!(text.contains("usai_http_requests_total 2"), "{text}");
    // State labels are lowercase, as `/_usai/status` spells them.
    assert!(
        text.contains("state=\"active\"") && !text.contains("state=\"Active\""),
        "{text}"
    );
    assert_eq!(
        status["http"]["rejections"],
        json!({ "route": 0, "validation": 1, "auth": 0, "capacity": 0, "draining": 0, "other": 0 })
    );
    assert_eq!(status["http"]["latency_cumulative"]["le"][0], json!(0.0005));
    // Not served on a host without the flag.
    let (status_code, _) = s.get("/_usai/metrics").await;
    assert_eq!(status_code, 404);
    let graph = usai_runtime::observability::render_graph(&s.runtime.active().unwrap().definition);
    assert!(graph.contains("POST /orders [request]\n   ├── cache.local/audit [lease]\n   └── hands work to → record [task]"), "{graph}");
    token.cancel();
    s.shutdown.cancel();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_status_token_guards_the_operator_surfaces_but_not_the_probes() {
    let Some(s) = start().await else { return };
    let host = HttpHost::new(
        Arc::clone(&s.runtime),
        HttpConfig {
            addr: ([127, 0, 0, 1], 0).into(),
            serve_status: true,
            serve_docs: true,
            status_token: Some("s3cret".into()),
            surfaces_off: vec!["metrics".into()],
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
    let base = format!("http://{addr}");
    // A surface switched off is a 404 like any unknown path, token or not.
    for auth in [None, Some("Bearer s3cret")] {
        let mut r = s.client.get(format!("{base}/_usai/metrics"));
        if let Some(a) = auth {
            r = r.header("authorization", a);
        }
        assert_eq!(r.send().await.unwrap().status(), 404, "metrics off");
    }
    for path in ["/_usai/status", "/_usai/openapi.json"] {
        let r = s.client.get(format!("{base}{path}")).send().await.unwrap();
        assert_eq!(r.status(), 401, "{path} without the token");
        let r = s
            .client
            .get(format!("{base}{path}"))
            .header("authorization", "Bearer wrong")
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 401, "{path} with a wrong token");
        let r = s
            .client
            .get(format!("{base}{path}"))
            .header("authorization", "Bearer s3cret")
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 200, "{path} with the token");
    }
    // Probes stay open (an orchestrator's healthcheck carries no header), and
    // so does the reference's shell, which asks for the token itself.
    for path in ["/_usai/live", "/_usai/ready", "/_usai/docs"] {
        let r = s.client.get(format!("{base}{path}")).send().await.unwrap();
        assert_eq!(r.status(), 200, "{path}");
    }
    // The application itself is untouched.
    let (status, _) = {
        let r = s
            .client
            .get(format!("{base}/counter"))
            .send()
            .await
            .unwrap();
        (r.status().as_u16(), r.text().await.unwrap())
    };
    assert_eq!(status, 200);
    token.cancel();
    s.shutdown.cancel();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_announced_surfaces_are_the_served_ones() {
    // The startup banner is the first thing read when verifying a hardened
    // deployment, and it used to advertise a surface `USAI_SURFACES_OFF` had
    // removed (round 16, an operator deploying 0.0.8 with `docs` off).
    let Some(s) = start().await else { return };
    let all = HttpHost::new(
        Arc::clone(&s.runtime),
        HttpConfig {
            serve_status: true,
            serve_docs: true,
            ..HttpConfig::default()
        },
    );
    assert_eq!(
        all.internal_surfaces(),
        vec![
            "/_usai/status",
            "/_usai/metrics",
            "/_usai/live",
            "/_usai/ready",
            "/_usai/docs"
        ]
    );
    let hardened = HttpHost::new(
        Arc::clone(&s.runtime),
        HttpConfig {
            serve_status: true,
            serve_docs: true,
            surfaces_off: vec!["docs".into(), "metrics".into()],
            ..HttpConfig::default()
        },
    );
    assert_eq!(
        hardened.internal_surfaces(),
        vec!["/_usai/status", "/_usai/live", "/_usai/ready"],
        "the banner must not name a surface that answers 404"
    );
    s.shutdown.cancel();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_public_listener_does_not_admit_that_a_protected_surface_exists() {
    // Production shape: the surfaces live on their own listener
    // (`--status-addr`) and a status token is set. The public listener then
    // serves none of them — and must say so the same way for all of them.
    // It used to answer 401 for /_usai/status and /_usai/metrics (the token
    // check ran before the listener's own routing) and 404 for the probes,
    // which told an unauthenticated caller both that this is a Usai runtime
    // and that there is an operator surface to come back for.
    let Some(s) = start().await else { return };
    let host = HttpHost::new(
        Arc::clone(&s.runtime),
        HttpConfig {
            addr: ([127, 0, 0, 1], 0).into(),
            serve_status: false,
            serve_docs: false,
            status_token: Some("s3cret".into()),
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
    let base = format!("http://{addr}");
    for path in [
        "/_usai/status",
        "/_usai/metrics",
        "/_usai/live",
        "/_usai/ready",
        "/_usai/docs",
        "/_usai/openapi.json",
    ] {
        for auth in [None, Some("Bearer s3cret")] {
            let mut r = s.client.get(format!("{base}{path}"));
            if let Some(a) = auth {
                r = r.header("authorization", a);
            }
            let r = r.send().await.unwrap();
            assert_eq!(r.status(), 404, "{path} (auth: {auth:?})");
            let body = r.text().await.unwrap();
            assert!(!body.contains("USAI_STATUS_TOKEN"), "{path}: {body}");
        }
    }
    token.cancel();
    s.shutdown.cancel();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_draining_host_fails_readiness_and_closes_connections_while_still_serving() {
    // The rolling-restart contract: between the stop signal and the
    // listener closing, a balancer must learn to route elsewhere and must
    // not reuse an idle keep-alive connection that is about to be closed
    // under it (the 502 the two-replica campaign measured).
    let Some(s) = start().await else { return };
    let host = HttpHost::new(
        Arc::clone(&s.runtime),
        HttpConfig {
            addr: ([127, 0, 0, 1], 0).into(),
            serve_status: true,
            ..HttpConfig::default()
        },
    );
    let draining = Arc::clone(&host);
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
    let base = format!("http://{addr}");
    let before = s
        .client
        .get(format!("{base}/_usai/ready"))
        .send()
        .await
        .unwrap();
    assert_eq!(before.status(), 200);
    assert_ne!(
        before.headers().get("connection").map(|v| v.as_bytes()),
        Some(&b"close"[..])
    );
    assert!(!draining.is_draining());

    draining.begin_draining();
    let ready = s
        .client
        .get(format!("{base}/_usai/ready"))
        .send()
        .await
        .unwrap();
    assert_eq!(ready.status(), 503);
    assert_eq!(ready.headers().get("connection").unwrap(), "close");
    assert_eq!(ready.json::<Value>().await.unwrap()["reason"], "draining");
    // Liveness and the application keep answering: the listener is open,
    // only the routing decision changed.
    let live = s
        .client
        .get(format!("{base}/_usai/live"))
        .send()
        .await
        .unwrap();
    assert_eq!(live.status(), 200);
    let app = s
        .client
        .get(format!("{base}/counter"))
        .send()
        .await
        .unwrap();
    assert_eq!(app.status(), 200);
    assert_eq!(app.headers().get("connection").unwrap(), "close");
    let rejected = s
        .client
        .get(format!("{base}/nowhere"))
        .send()
        .await
        .unwrap();
    assert_eq!(rejected.status(), 404);
    assert_eq!(
        rejected.headers().get("connection").unwrap(),
        "close",
        "replies decided before a world close too"
    );
    token.cancel();
    s.shutdown.cancel();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_cookie_scheme_reads_the_cookie_and_login_sets_two() {
    let Some(s) = start().await else { return };
    let login = s
        .client
        .post(format!("{}/login", s.base))
        .send()
        .await
        .unwrap();
    assert_eq!(login.status(), 200);
    let cookies: Vec<&str> = login
        .headers()
        .get_all("set-cookie")
        .iter()
        .map(|v| v.to_str().unwrap())
        .collect();
    assert_eq!(cookies.len(), 2, "{cookies:?}");
    assert_eq!(
        cookies[0],
        "sid=s3ss10n; Max-Age=3600; Path=/; Secure; HttpOnly; SameSite=Lax"
    );
    assert_eq!(cookies[1], "theme=dark; Path=/; Secure; SameSite=Lax");

    let (status, body) = s.get("/me/cookie").await;
    assert_eq!(status, 401);
    assert_eq!(body["error"]["message"], "missing sid cookie");
    let r = s
        .client
        .get(format!("{}/me/cookie", s.base))
        .header("cookie", "theme=dark; sid=s3ss10n")
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    let principal = r.json::<Value>().await.unwrap();
    assert_eq!(principal["userId"], "u1");
    // The resolver leased the scheme's own resource (`sessions`), which the
    // route never listed: the scheme's resources ride along.
    assert_eq!(principal["seen"], 1, "{principal}");
    let r = s
        .client
        .get(format!("{}/me/cookie", s.base))
        .header("cookie", "sid=wrong")
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 401);
    s.shutdown.cancel();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn every_request_has_an_id_the_world_sees_and_the_response_carries() {
    let Some(s) = start().await else { return };
    // Minted when the client sends none: a UUID, on the response and in ctx.
    let r = s
        .client
        .get(format!("{}/request-id", s.base))
        .send()
        .await
        .unwrap();
    let echoed = r
        .headers()
        .get("x-request-id")
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    assert_eq!(echoed.len(), 36, "{echoed}");
    assert_eq!(r.json::<Value>().await.unwrap()["id"], echoed);
    // Catch-all segments: the remainder of the path is one param, and an
    // OPTIONS catch-all answers every preflight (a declared route wins over
    // the runtime's 405).
    let r = s
        .client
        .get(format!("{}/files/a/b/c.txt", s.base))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    assert_eq!(
        r.json::<Value>().await.unwrap(),
        json!({ "path": "a/b/c.txt" })
    );
    let r = s
        .client
        .request(reqwest::Method::OPTIONS, format!("{}/users/{UUID}", s.base))
        .header("origin", "http://localhost:5173")
        .header("access-control-request-method", "GET")
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 204, "{:?}", r.headers());
    assert_eq!(
        r.headers().get("access-control-allow-origin").unwrap(),
        "http://localhost:5173"
    );
    // Literal routes still win over the catch-all for their own method.
    let r = s
        .client
        .get(format!("{}/request-id", s.base))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    // A conditional GET: 200 with the handler's ETag, then a bodiless 304
    // that the response contract never had to declare.
    let r = s
        .client
        .get(format!("{}/cached", s.base))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    assert_eq!(r.headers().get("etag").unwrap(), "\"v1\"");
    assert_eq!(r.json::<Value>().await.unwrap(), json!({ "v": 1 }));
    let r = s
        .client
        .get(format!("{}/cached", s.base))
        .header("if-none-match", "\"v1\"")
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 304);
    assert_eq!(r.headers().get("etag").unwrap(), "\"v1\"");
    assert!(
        r.headers().get("content-type").is_none(),
        "{:?}",
        r.headers()
    );
    assert_eq!(r.bytes().await.unwrap().len(), 0);
    // A signed bearer token round-trips inside the world.
    let r: Value = s
        .client
        .get(format!("{}/token", s.base))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(r["sub"], "u1", "{r}");
    assert!(r["exp"].as_u64().is_some_and(|e| e > 1_700_000_000), "{r}");
    assert_eq!(r["tampered"], Value::Null);
    assert!(r["token"].as_str().unwrap().contains('.'));
    // The id follows the work the request hands off: the task it invokes
    // sees it, and so does the one it dispatches.
    let r: Value = s
        .client
        .get(format!("{}/request-id/hand-off", s.base))
        .header("x-request-id", "trace-handoff-1")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(r["id"], "trace-handoff-1");
    assert_eq!(r["invoked"], "trace-handoff-1", "{r}");
    let mut last = Value::Null;
    for _ in 0..50 {
        last = s
            .client
            .get(format!("{}/request-id/last", s.base))
            .send()
            .await
            .unwrap()
            .json::<Value>()
            .await
            .unwrap()["last"]
            .clone();
        if last == "trace-handoff-1" {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert_eq!(
        last, "trace-handoff-1",
        "the dispatched task saw the request id"
    );
    // The client's, when sane.
    let r = s
        .client
        .get(format!("{}/request-id", s.base))
        .header("x-request-id", "trace-abc.123")
        .send()
        .await
        .unwrap();
    assert_eq!(r.headers().get("x-request-id").unwrap(), "trace-abc.123");
    assert_eq!(r.json::<Value>().await.unwrap()["id"], "trace-abc.123");
    // Replaced when not: too long, or not printable ASCII.
    let long = "x".repeat(200);
    let r = s
        .client
        .get(format!("{}/request-id", s.base))
        .header("x-request-id", long.as_str())
        .send()
        .await
        .unwrap();
    assert_ne!(
        r.headers().get("x-request-id").unwrap().to_str().unwrap(),
        long
    );
    // Refusals before a world carry one too.
    let r = s
        .client
        .get(format!("{}/nowhere", s.base))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 404);
    assert!(r.headers().get("x-request-id").is_some());
    s.shutdown.cancel();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn application_headers_are_on_every_response_and_a_handler_wins() {
    let Some(s) = start().await else { return };
    for path in ["/request-id", "/nowhere"] {
        let r = s
            .client
            .get(format!("{}{path}", s.base))
            .send()
            .await
            .unwrap();
        assert_eq!(
            r.headers().get("x-content-type-options").unwrap(),
            "nosniff",
            "{path}"
        );
        assert_eq!(
            r.headers().get("x-frame-options").unwrap(),
            "DENY",
            "{path}"
        );
    }
    let r = s
        .client
        .get(format!("{}/framed", s.base))
        .send()
        .await
        .unwrap();
    assert_eq!(r.headers().get("x-frame-options").unwrap(), "SAMEORIGIN");
    assert_eq!(
        r.headers().get("x-content-type-options").unwrap(),
        "nosniff"
    );
    s.shutdown.cancel();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_undeclared_hand_off_still_happens_and_is_said_out_loud() {
    let Some(s) = start().await else { return };
    // The declaration is documentation, not permission: refusing would break
    // running applications. But `usai graph` and the reference read the
    // definition, so an edge nobody declared has to be reported somewhere.
    let (status, body) = s.get("/undeclared-dispatch").await;
    assert_eq!(status, 200, "{body}");
    assert!(
        body["dispatched"]
            .as_str()
            .is_some_and(|id| id.contains("record")),
        "the hand-off must happen: {body}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_monotonic_clock_starts_with_the_world_and_measures_elapsed_time() {
    let Some(s) = start().await else { return };
    let (status, body) = s.get("/clocks").await;
    assert_eq!(status, 200, "{body}");
    let at_start = body["perfAtStart"].as_f64().expect("perfAtStart");
    let elapsed = body["perfElapsed"].as_f64().expect("perfElapsed");
    // A world is a fresh context: its monotonic clock starts near zero, not
    // at "however long ago the image was built".
    assert!(
        at_start < 1_000.0,
        "the world's clock started at {at_start} ms"
    );
    assert!(
        (55.0..5_000.0).contains(&elapsed),
        "a 60 ms sleep measured {elapsed} ms on the monotonic clock"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn declared_string_formats_are_enforced_at_both_halves_of_the_boundary() {
    let Some(s) = start().await else { return };
    let valid = json!({ "url": "https://example.dev/x", "email": "a@b.co", "id": "3f2504e0-4f89-11d3-9a0c-0305e82c3301" });
    let (status, body) = s.post_json("/formats", valid.clone()).await;
    assert_eq!(status, 200, "{body}");
    // Rejected by the host, before a world: the JSON Schema says `uri`.
    let mut bad = valid.clone();
    bad["url"] = json!("not-a-url");
    let (status, body) = s.post_json("/formats", bad).await;
    assert_eq!(status, 400, "{body}");
    assert_eq!(body["error"]["details"]["issues"][0]["path"], json!("/url"));
    // Accepted by JSON Schema's `uri` (a scheme with an empty path is a URI)
    // and rejected by `z.url()`: the world's finalizer must run the node's
    // own check, or a route would accept what its own schema refuses.
    let mut scheme_only = valid.clone();
    scheme_only["url"] = json!("ftp:");
    let (status, body) = s.post_json("/formats", scheme_only).await;
    assert_eq!(
        status, 400,
        "the world must apply the library's own format check: {body}"
    );
    for (field, value) in [("email", "nope"), ("id", "xyz")] {
        let mut bad = valid.clone();
        bad[field] = json!(value);
        let (status, body) = s.post_json("/formats", bad).await;
        assert_eq!(status, 400, "{field} was accepted: {body}");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_multi_megabyte_body_decodes_within_the_cpu_slice() {
    let Some(s) = start().await else { return };
    // 3 MB of mixed ASCII and two-byte UTF-8, one decode + one base64
    // round trip in the guest; used to fault on the 5 s synchronous slice.
    let text: String = "ab\u{e9}".repeat(600_000);
    let started = std::time::Instant::now();
    let r = s
        .client
        .post(format!("{}/decode", s.base))
        .header("content-type", "application/octet-stream")
        .body(text.clone())
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200, "{}", r.text().await.unwrap());
    let body = r.json::<Value>().await.unwrap();
    assert_eq!(body["bytes"], text.len());
    assert_eq!(body["chars"], text.chars().count());
    assert_eq!(body["roundtrip"], 1_000_000);
    // The guard against the regression this test exists for is the 200
    // above: a decode that exceeds the world's synchronous CPU slice faults
    // the world and answers 5xx. The wall clock is a debug build sharing a
    // machine with the rest of `make check`, so it only has to show the
    // request did not hang — a tighter bound here measures the load on the
    // runner, not the runtime (`bench`/`profile_bundles` measure that, in
    // release).
    assert!(
        started.elapsed() < std::time::Duration::from_secs(15),
        "{:?}",
        started.elapsed()
    );
    s.shutdown.cancel();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_stream_declares_its_media_type() {
    let Some(s) = start().await else { return };
    let r = s
        .client
        .get(format!("{}/export.csv", s.base))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    assert_eq!(r.headers().get("content-type").unwrap(), "text/csv");
    assert_eq!(r.text().await.unwrap(), "id,name\n1,Ayu\n");
    let doc = usai_runtime::openapi::generate_with(
        &s.runtime.active().unwrap().definition,
        s.runtime.config(),
        usai_runtime::openapi::Profile::Internal,
    );
    let op = &doc["paths"]["/export.csv"]["get"];
    assert!(
        op["responses"]["200"]["content"].get("text/csv").is_some(),
        "{op}"
    );
    assert!(
        op["description"]
            .as_str()
            .unwrap()
            .contains("Chunks are text/csv"),
        "{op}"
    );
    s.shutdown.cancel();
}
