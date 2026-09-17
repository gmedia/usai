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
    if !root.join("node_modules/usai").exists() {
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
        |name| (name == "GREETING").then(|| "hi".to_owned()),
    );
    let rev = runtime.install(out.definition).await.unwrap();
    runtime.activate(rev.id).await.unwrap();
    let host = HttpHost::new(
        Arc::clone(&runtime),
        HttpConfig {
            addr: ([127, 0, 0, 1], 0).into(),
            expose_diagnostics: false,
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
    let before = s.runtime.ledger().gauges.snapshot();
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
    let (status, body) = s.get("/bad-shape").await;
    assert_eq!(status, 500, "{body}");
    assert_eq!(body["error"]["code"], "response_contract_violation");
    s.shutdown.cancel();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn detached_work_is_reported_and_the_response_still_commits() {
    let Some(s) = start().await else { return };
    let (status, body) = s.get("/detach").await;
    assert_eq!(status, 200);
    assert_eq!(body["ok"], true);
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(
        s.runtime.ledger().gauges.snapshot().detached_work_detected,
        1
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
    let doc = usai_runtime::openapi::generate(&rev.definition);
    assert_eq!(doc["openapi"], "3.1.0");
    assert_eq!(doc["info"]["title"], "http-fixture");
    assert_eq!(doc["info"]["version"], rev.definition.identity());
    let get_user = &doc["paths"]["/users/{id}"]["get"];
    assert_eq!(get_user["tags"], json!(["users"]));
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
    let webhook = &doc["paths"]["/webhook"]["post"];
    assert_eq!(
        webhook["x-usai-raw"], true,
        "raw endpoints are opaque, not invented"
    );
    assert!(webhook.get("parameters").is_none());
    assert!(
        doc.to_string().find("$schema").is_none_or(|_| false)
            || !doc["paths"].to_string().contains("\"$schema\"")
    );
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
    // Not served on a host without the flag.
    let (status_code, _) = s.get("/_usai/metrics").await;
    assert_eq!(status_code, 404);
    let graph = usai_runtime::observability::render_graph(&s.runtime.active().unwrap().definition);
    assert!(graph.contains("POST /orders [request]\n   ├── cache.local/audit [lease]\n   └── dispatch → record [task]"), "{graph}");
    token.cancel();
    s.shutdown.cancel();
}
