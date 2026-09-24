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
    if !hello.join("node_modules/@sakaladev/usai").exists()
        || !fixture.join("node_modules/@sakaladev/usai").exists()
    {
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
        |name| (name == "UPSTREAM_URL").then(|| "http://127.0.0.1:9/".to_owned()),
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
async fn held_revisions_are_bounded_and_ids_are_accepted_bare_or_prefixed() {
    let Some((runtime, hello, _fixture_dir, base, shutdown, client)) = setup().await else {
        return;
    };
    let auth = |r: reqwest::RequestBuilder| r.bearer_auth("s3cret");
    let max = runtime.config().max_revisions;
    // One is active already; installing up to the bound works, one more is refused.
    let mut last = 0;
    for i in 1..max {
        let r = auth(client.post(format!("{base}/revisions")))
            .json(&json!({ "artifact": hello.to_string_lossy() }))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 201, "install {i}");
        last = r.json::<Value>().await.unwrap()["id"].as_u64().unwrap();
    }
    let r = auth(client.post(format!("{base}/revisions")))
        .json(&json!({ "artifact": hello.to_string_lossy() }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 409);
    let body: Value = r.json().await.unwrap();
    assert_eq!(body["error"]["code"], "too_many_revisions");
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("DELETE /revisions")
    );
    // The JSON carries a bare id; both spellings address the revision.
    let r = auth(client.delete(format!("{base}/revisions/{last}")))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200, "bare id");
    let r = auth(client.delete(format!("{base}/revisions/rev{}", last - 1)))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200, "prefixed id");
    shutdown.cancel();
    runtime.shutdown().await;
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
    //
    // This fixture is a *different* application from the one the runtime
    // started with, which a deploy refuses by default — a template that
    // interpolated the wrong release directory would otherwise replace one
    // service with another, with a plain 200 throughout. Here the swap is
    // the point of the test, so it says so.
    let r = auth(client.post(format!("{base}/revisions/rev{id}/activate")))
        .json(&json!({ "allowApplicationChange": true }))
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
    // The replaced revision is draining, or — with nothing in flight — has
    // already retired itself.
    assert!(
        states.contains(&(1, "draining".into())) || !states.iter().any(|(i, _)| *i == 1),
        "{states:?}"
    );
    assert!(states.contains(&(id, "active".into())));

    // An explicit drain is still fine: it waits for the same thing (or
    // answers 404 when the revision already retired itself).
    let r = auth(client.post(format!("{base}/revisions/rev1/drain")))
        .send()
        .await
        .unwrap();
    assert!(r.status() == 200 || r.status() == 404, "{}", r.status());
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

    // Rollback: the previous artifact is installed again (a retired revision
    // is never re-activated; the artifact is the identity, ADR-0005) and
    // activated, and the runtime serves it again.
    let r = auth(client.post(format!("{base}/revisions")))
        .json(&json!({ "artifact": _hello.to_string_lossy() }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 201);
    let back: Value = r.json().await.unwrap();
    assert_eq!(back["application"], "hello");
    let back_id = back["id"].as_u64().unwrap();
    // Back to the other application, again deliberately.
    let r = auth(client.post(format!("{base}/revisions/rev{back_id}/activate")))
        .json(&json!({ "allowApplicationChange": true }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200, "{}", r.text().await.unwrap());
    let health: Value = auth(client.get(format!("{base}/health")))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(health["active"]["application"], "hello", "rolled back");
    let r = auth(client.post(format!("{base}/revisions/rev{id}/drain")))
        .send()
        .await
        .unwrap();
    assert!(
        r.status() == 200 || r.status() == 404,
        "retired itself or drains now: {}",
        r.status()
    );

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
        let rt = Runtime::with_env(engine, RuntimeConfig::default(), |name| {
            (name == "UPSTREAM_URL").then(|| "http://127.0.0.1:9/".to_owned())
        });
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

/// The control surface is reachable from the operator's own browser, and
/// `POST /stop` needs no body — which makes it a CORS **simple request**, a
/// thing any page can send with no preflight and no consent. With the
/// token-less loopback configuration this surface permits, that was an
/// unauthenticated remote kill. Two independent closures: any request
/// carrying `Origin` is refused, and a mutating request whose media type is
/// one a browser may send without a preflight is refused.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_web_page_cannot_drive_a_deployment() {
    let Some((runtime, hello, _fixture_dir, base, shutdown, client)) = setup().await else {
        return;
    };
    let auth = |r: reqwest::RequestBuilder| r.bearer_auth("s3cret");
    for (name, r) in [
        (
            "stop",
            auth(client.post(format!("{base}/stop"))).header("origin", "https://evil.example"),
        ),
        (
            "install",
            auth(client.post(format!("{base}/revisions")))
                .header("origin", "https://evil.example")
                .json(&json!({ "artifact": hello.to_string_lossy() })),
        ),
    ] {
        let r = r.send().await.unwrap();
        assert_eq!(r.status(), 403, "{name}");
        let body: Value = r.json().await.unwrap();
        assert_eq!(body["error"]["code"], "cross_origin_refused", "{name}");
    }
    // A form POST is the other half of the same trick.
    let r = auth(client.post(format!("{base}/stop")))
        .header("content-type", "application/x-www-form-urlencoded")
        .body("")
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 415);
    // A deploy script is unaffected: JSON, or no body at all.
    let r = auth(client.get(format!("{base}/health")))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    shutdown.cancel();
    runtime.shutdown().await;
}

/// A control plane retries on timeout. Two of this API's verbs could not
/// survive that: a retried `activate` arriving while the previous revision
/// is still `draining` is **the documented rollback**, so it silently
/// reverted whatever deployed in between and answered 200; and a retried
/// `install` became a second revision holding a second compiled image, with
/// nothing to tell it from the first.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_deployer_that_retries_cannot_undo_someone_elses_deploy() {
    let Some((runtime, hello, _fixture_dir, base, shutdown, client)) = setup().await else {
        return;
    };
    let auth = |r: reqwest::RequestBuilder| r.bearer_auth("s3cret");
    let active = runtime.active().unwrap().id.0;
    // `ifAbsent` makes the install the same answer however many times it is
    // asked: the retry adopts the revision the first call made.
    let first: Value = auth(client.post(format!("{base}/revisions")))
        .json(&json!({ "artifact": hello.to_string_lossy(), "ifAbsent": true }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(first["installed"], true, "{first}");
    let retry: Value = auth(client.post(format!("{base}/revisions")))
        .json(&json!({ "artifact": hello.to_string_lossy(), "ifAbsent": true }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        retry["id"], first["id"],
        "a retry must not make a second revision"
    );
    assert_eq!(
        retry["installed"], false,
        "and it must say which it was: {retry}"
    );

    // The compare-and-swap: this deployer saw `active` and says so.
    let id = first["id"].as_u64().unwrap();
    let r = auth(client.post(format!("{base}/revisions/rev{id}/activate")))
        .json(&json!({ "expectedPrevious": active + 999, "allowApplicationChange": true }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 409, "the active revision moved under it");
    let body: Value = r.json().await.unwrap();
    assert_eq!(body["error"]["code"], "active_revision_moved");
    let r = auth(client.post(format!("{base}/revisions/rev{id}/activate")))
        .json(&json!({ "expectedPrevious": active, "allowApplicationChange": true }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200, "{}", r.text().await.unwrap());
    shutdown.cancel();
    runtime.shutdown().await;
}

/// **The rollback handle the document promised is not one.** The state table
/// said `activate` again on a `draining` revision is a rollback, and the
/// deploy recipe's last line said the same — so an operator reads it, does
/// not keep the previous artifact directory mounted, and discovers on a bad
/// deploy that every request is a 500, readiness is 200 and the rollback is
/// a `404`. A revision with nothing in flight drains and retires at once;
/// the window exists only while something long-running holds it.
///
/// What always works is installing the previous artifact again, which is
/// why `CONTROL-API.md` now says to keep its directory.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_replaced_idle_revision_is_not_a_rollback_handle() {
    let Some((runtime, hello, _fixture_dir, base, shutdown, client)) = setup().await else {
        return;
    };
    let auth = |r: reqwest::RequestBuilder| r.bearer_auth("s3cret");
    let replaced = runtime.active().unwrap().id.0;
    let install: Value = auth(client.post(format!("{base}/revisions")))
        .json(&json!({ "artifact": hello.to_string_lossy(), "ifAbsent": true }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let next = install["id"].as_u64().unwrap();
    let r = auth(client.post(format!("{base}/revisions/rev{next}/activate")))
        .json(&json!({ "allowApplicationChange": true }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200, "{}", r.text().await.unwrap());

    // Nothing was in flight, so the revision it replaced is already gone.
    let r = auth(client.post(format!("{base}/revisions/rev{replaced}/activate")))
        .json(&json!({ "allowApplicationChange": true }))
        .send()
        .await
        .unwrap();
    assert_eq!(
        r.status(),
        404,
        "the documented rollback answered: {}",
        r.text().await.unwrap()
    );

    // And the rollback the document now describes does work: the artifact
    // directory is the handle, so it has to still be on disk.
    let again: Value = auth(client.post(format!("{base}/revisions")))
        .json(&json!({ "artifact": hello.to_string_lossy(), "ifAbsent": true }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let back = again["id"].as_u64().unwrap();
    let r = auth(client.post(format!("{base}/revisions/rev{back}/activate")))
        .json(&json!({ "allowApplicationChange": true }))
        .send()
        .await
        .unwrap();
    assert_eq!(
        r.status(),
        200,
        "reinstall-and-activate is the rollback that always works: {}",
        r.text().await.unwrap()
    );
    shutdown.cancel();
    runtime.shutdown().await;
}

/// Draining the only revision there is takes the runtime out of service with
/// nothing to put back: 503 for everything, and the only way back is a fresh
/// install with an artifact path this process no longer remembers. One call,
/// a millisecond, no confirmation — and `activate` already drains the
/// revision it replaces, so a deployer never needs this in a deploy.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn draining_the_only_revision_is_refused() {
    let Some((runtime, _hello, _fixture_dir, base, shutdown, client)) = setup().await else {
        return;
    };
    let auth = |r: reqwest::RequestBuilder| r.bearer_auth("s3cret");
    let active = runtime.active().unwrap().id.0;
    let r = auth(client.post(format!("{base}/revisions/rev{active}/drain")))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 409, "{}", r.text().await.unwrap());
    let body: Value = r.json().await.unwrap();
    assert_eq!(body["error"]["code"], "would_stop_serving");
    assert!(runtime.active().is_ok(), "it must still be serving");
    shutdown.cancel();
    runtime.shutdown().await;
}

/// `activate` answers 200 for a revision that cannot serve a single
/// request: an unapplied migration, a schema drift, a resource the
/// developer forgot to list are all outside the manifest the runtime
/// checks — and by then the previous revision is already draining away.
/// `POST /revisions/{id}/verify` runs one workload on the revision while it
/// is still `installed`, against the resources it will actually use, so a
/// deployer learns before production does.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_revision_can_be_exercised_before_it_takes_traffic() {
    let Some((runtime, _hello, fixture_dir, base, shutdown, client)) = setup().await else {
        return;
    };
    let auth = |r: reqwest::RequestBuilder| r.bearer_auth("s3cret");
    let serving_before = runtime.active().unwrap().id;
    let installed: Value = auth(client.post(format!("{base}/revisions")))
        .json(&json!({ "artifact": fixture_dir.to_string_lossy() }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let id = installed["id"].as_u64().unwrap();
    assert_eq!(installed["state"], "installed");

    // A task that exists runs, on a revision that is serving nothing.
    let r = auth(client.post(format!("{base}/revisions/rev{id}/verify")))
        .json(&json!({ "kind": "task", "name": "record", "input": { "what": "verify" } }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200, "{}", r.text().await.unwrap());
    let body: Value = r.json().await.unwrap();
    assert_eq!(body["ok"], true, "{body}");
    assert_eq!(body["termination"], "completed", "{body}");

    // Nothing was promoted: the revision that was serving still is.
    assert_eq!(runtime.active().unwrap().id, serving_before);
    let revisions: Value = auth(client.get(format!("{base}/revisions")))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let row = revisions["revisions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == id)
        .expect("the revision is still listed");
    assert_eq!(row["state"], "installed", "{row}");

    // A workload that does not exist is named, not a 500.
    let r = auth(client.post(format!("{base}/revisions/rev{id}/verify")))
        .json(&json!({ "kind": "task", "name": "does-not-exist" }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 404, "{}", r.text().await.unwrap());

    // A refusal on a documented endpoint says what the endpoint wanted. With
    // no body at all this answered `EOF while parsing a value at line 1
    // column 0` — a raw serde message, on the same page that tells operators
    // bodyless POSTs still work, for the commonest mistake there is here.
    let r = auth(client.post(format!("{base}/revisions/rev{id}/verify")))
        .header("content-type", "application/json")
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 400);
    let body: Value = r.json().await.unwrap();
    let message = body["error"]["message"].as_str().unwrap_or_default();
    assert!(
        !message.contains("EOF while parsing"),
        "the parser's message is not an answer: {message}"
    );
    assert!(
        message.contains("needs a JSON body") && message.contains("kind"),
        "the refusal has to name what this endpoint takes: {message}"
    );
    shutdown.cancel();
    runtime.shutdown().await;
}
