//! D11 acceptance: connection-bound workloads — streams and sockets.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::Message;
use tokio_util::sync::CancellationToken;
use usai_runtime::build::{BuildOptions, build};
use usai_runtime::http::{HttpConfig, HttpHost, serve};
use usai_runtime::*;

struct Server {
    base: String,
    ws: String,
    runtime: Arc<Runtime>,
    shutdown: CancellationToken,
    client: reqwest::Client,
}

async fn start() -> Option<Server> {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_test_writer()
        .try_init();
    if std::process::Command::new("node")
        .arg("--version")
        .output()
        .map(|o| !o.status.success())
        .unwrap_or(true)
    {
        return None;
    }
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/http-app");
    if !root.join("node_modules/@sakaladev/usai").exists() {
        return None;
    }
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let out_dir = std::env::temp_dir().join(format!(
        "usai-conn-test-{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    ));
    let engine = usai_runtime::engine::from_env(64).unwrap();
    let out = build(
        engine.as_ref(),
        &BuildOptions {
            out_dir,
            ..BuildOptions::for_project(&root)
        },
    )
    .await
    .expect("fixture builds");
    let runtime = Runtime::with_env(
        engine,
        RuntimeConfig {
            cron_scheduler: false,
            drain_timeout: Duration::from_secs(5),
            ..RuntimeConfig::default()
        },
        |name| (name == "UPSTREAM_URL").then(|| "http://127.0.0.1:9/".to_owned()),
    );
    let rev = runtime.install(out.definition).await.unwrap();
    runtime.activate(rev.id).await.unwrap();
    let host = HttpHost::new(
        Arc::clone(&runtime),
        HttpConfig {
            addr: ([127, 0, 0, 1], 0).into(),
            expose_diagnostics: true,
            serve_status: true,
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
        .unwrap()
    });
    let addr = rx.await.unwrap();
    Some(Server {
        base: format!("http://{addr}"),
        ws: format!("ws://{addr}"),
        runtime,
        shutdown,
        client: reqwest::Client::new(),
    })
}

/// Live worlds that are not the fixture's own service: the service is a
/// live world by design for the revision's lifetime.
async fn live_connection_worlds(rt: &Runtime) -> u64 {
    let services = rt
        .active()
        .map(|r| {
            r.services()
                .iter()
                .filter(|s| s.state == usai_runtime::workloads::services::ServiceState::Running)
                .count() as u64
        })
        .unwrap_or(0);
    rt.ledger()
        .gauges
        .snapshot()
        .live_worlds
        .saturating_sub(services)
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

async fn finish(s: Server) {
    s.runtime.shutdown().await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    let g = s.runtime.ledger().gauges.snapshot();
    assert_eq!(g.live_worlds, 0, "{g:?}");
    assert_eq!(g.live_ops, 0, "{g:?}");
    s.shutdown.cancel();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stream_lives_until_the_handler_returns() {
    let Some(s) = start().await else { return };
    let response = s
        .client
        .get(format!("{}/events?n=3", s.base))
        .send()
        .await
        .unwrap();
    if response.status() != 200 {
        panic!("{}", response.text().await.unwrap());
    }
    assert_eq!(
        response.headers().get("content-type").unwrap(),
        "text/event-stream"
    );
    assert_eq!(response.headers().get("x-stream").unwrap(), "yes");
    assert_eq!(response.headers().get("x-usai-lifetime").unwrap(), "stream");
    let body = response.text().await.unwrap();
    let events: Vec<&str> = body.split("\n\n").filter(|e| !e.is_empty()).collect();
    assert_eq!(events.len(), 4, "{body}");
    assert_eq!(events[0], "event: tick\ndata: {\"i\":1}");
    assert_eq!(events[3], "event: done\ndata: {\"total\":3}");
    // A stream handler that never sends is an ordinary response.
    let (status, body) = {
        let r = s
            .client
            .get(format!("{}/no-send", s.base))
            .send()
            .await
            .unwrap();
        (r.status().as_u16(), r.json::<Value>().await.unwrap())
    };
    assert_eq!(status, 200);
    assert_eq!(body, json!({ "nothing": "sent" }));
    // Boundary validation still happens before the world.
    let r = s
        .client
        .get(format!("{}/events?n=1000", s.base))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 400);
    // Accounting: a committed stream counts as a stream and a 2xx, never
    // as a server error.
    let status: Value = s
        .client
        .get(format!("{}/_usai/status", s.base))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(status["http"]["streams"], json!(1), "{status}");
    assert_eq!(status["http"]["responses_5xx"], json!(0), "{status}");
    finish(s).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn client_disconnect_ends_the_stream_world() {
    let Some(s) = start().await else { return };
    // A raw client, so the disconnect is an unambiguous TCP close.
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let addr = s.base.trim_start_matches("http://").to_owned();
    let mut tcp = tokio::net::TcpStream::connect(&addr).await.unwrap();
    tcp.write_all(format!("GET /endless HTTP/1.1\r\nHost: {addr}\r\n\r\n").as_bytes())
        .await
        .unwrap();
    let mut buf = vec![0u8; 4096];
    let n = tcp.read(&mut buf).await.unwrap();
    let head = String::from_utf8_lossy(&buf[..n]).to_string();
    assert!(head.starts_with("HTTP/1.1 200"), "{head}");
    drop(tcp);
    let started = std::time::Instant::now();
    loop {
        if live_connection_worlds(&s.runtime).await == 0 {
            break;
        }
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "stream world outlived its client: {:?}",
            s.runtime.ledger().gauges.snapshot()
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    // The client leaving is how every event stream ends: not a failed
    // stream (no ERROR line, no counter), just a world cancelled with its
    // connection.
    tokio::time::sleep(Duration::from_millis(100)).await;
    let status: Value = s
        .client
        .get(format!("{}/_usai/status", s.base))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(status["http"]["streams_failed"], json!(0), "{status}");
    assert_eq!(status["http"]["streams"], json!(1), "{status}");
    finish(s).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn drain_stops_an_endless_stream_gracefully() {
    let Some(s) = start().await else { return };
    let response = s
        .client
        .get(format!("{}/endless", s.base))
        .send()
        .await
        .unwrap();
    let mut body = response.bytes_stream();
    let _ = body.next().await.unwrap().unwrap();
    let rev = s.runtime.active().unwrap();
    let b = s
        .runtime
        .install(Arc::clone(&rev.definition))
        .await
        .unwrap();
    s.runtime.activate(b.id).await.unwrap();
    let started = std::time::Instant::now();
    s.runtime.drain(rev.id).await.unwrap();
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "graceful stop should end the stream loop promptly"
    );
    // The body ends (handler returned after the signal).
    let mut rest = 0;
    while let Some(Ok(_)) = body.next().await {
        rest += 1;
    }
    assert!(rest < 200);
    finish(s).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_authenticated_socket_refuses_before_the_upgrade_and_takes_a_browser_credential() {
    let Some(s) = start().await else { return };
    // No credential: the upgrade is refused with a status, not a 101 that
    // closes at once — a browser can tell "log in again" from "retry".
    let refused = s
        .client
        .get(format!("{}/private-chat", s.base))
        .header("connection", "upgrade")
        .header("upgrade", "websocket")
        .header("sec-websocket-version", "13")
        .header("sec-websocket-key", "dGhlIHNhbXBsZSBub25jZQ==")
        .send()
        .await
        .unwrap();
    assert_eq!(refused.status(), 401, "{}", refused.text().await.unwrap());
    // A wrong token: the resolver's own 401.
    let wrong = s
        .client
        .get(format!("{}/private-chat", s.base))
        .header("connection", "upgrade")
        .header("upgrade", "websocket")
        .header("sec-websocket-version", "13")
        .header("sec-websocket-key", "dGhlIHNhbXBsZSBub25jZQ==")
        .header("authorization", "Bearer nope")
        .send()
        .await
        .unwrap();
    assert_eq!(wrong.status(), 401);
    let body: Value = wrong.json().await.unwrap();
    assert_eq!(body["error"]["message"], "bad token");
    // The browser form: `new WebSocket(url, ["bearer", token])`.
    let mut request =
        tokio_tungstenite::tungstenite::client::IntoClientRequest::into_client_request(format!(
            "{}/private-chat",
            s.ws
        ))
        .unwrap();
    request
        .headers_mut()
        .insert("sec-websocket-protocol", "bearer, secret".parse().unwrap());
    let (mut ws, response) = tokio_tungstenite::connect_async(request)
        .await
        .expect("upgrade");
    assert_eq!(response.status(), 101);
    assert_eq!(
        response.headers().get("sec-websocket-protocol").unwrap(),
        "bearer",
        "the selected subprotocol is echoed, as browsers require"
    );
    assert_eq!(recv_json(&mut ws).await, json!({ "user": "u1" }));
    // Accounting: the refusals were 401s, the connection an upgrade.
    let status: Value = s
        .client
        .get(format!("{}/_usai/status", s.base))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        status["http"]["by_workload"]["socket:/private-chat"]["4xx"],
        json!(2),
        "{status}"
    );
    assert_eq!(
        status["http"]["by_workload"]["socket:/private-chat"]["2xx"],
        json!(1),
        "{status}"
    );
    finish(s).await;
}

async fn connect(
    s: &Server,
    user: &str,
) -> tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>> {
    let (ws, response) = tokio_tungstenite::connect_async(format!("{}/chat?user={user}", s.ws))
        .await
        .expect("upgrade");
    assert_eq!(response.status(), 101);
    ws
}

async fn recv_json(
    ws: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
) -> Value {
    loop {
        match tokio::time::timeout(Duration::from_secs(3), ws.next())
            .await
            .expect("frame in time")
        {
            Some(Ok(Message::Text(t))) => return serde_json::from_str(&t).unwrap(),
            Some(Ok(Message::Close(f))) => {
                return json!({ "close": f.map(|f| f.reason.to_string()) });
            }
            Some(Ok(_)) => continue,
            other => panic!("unexpected: {other:?}"),
        }
    }
}

/// A socket gets no *default* deadline. One it declares must reach the
/// manifest and end the world — the runtime was ready to enforce it and the
/// SDK dropped it, so "a `timeout:` declared on a stream, a socket or a
/// service is honoured" was true for one of the three, and `usai inspect`
/// told a socket that declared one to declare one.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_declared_timeout_bounds_a_socket() {
    let Some(s) = start().await else { return };
    let started = std::time::Instant::now();
    let (mut ws, response) = tokio_tungstenite::connect_async(format!("{}/push?who=bounded", s.ws))
        .await
        .expect("upgrade");
    assert_eq!(response.status(), 101);
    // The handler pushes every 50 ms for up to 400 iterations (20 s); the
    // declaration says 600 ms.
    let mut frames = 0;
    // A five-second bound on each frame: when the deadline ends the socket
    // the stream closes, and if it merely went quiet the timeout catches it
    // rather than hanging the test.
    while let Ok(Some(Ok(message))) = tokio::time::timeout(Duration::from_secs(5), ws.next()).await
    {
        if message.is_text() {
            frames += 1;
        }
        if message.is_close() {
            break;
        }
    }
    let took = started.elapsed();
    assert!(
        took < Duration::from_secs(5),
        "the socket ran {took:?} past a declared 600 ms"
    );
    assert!(frames > 0, "nothing was pushed, so this proves nothing");
    // And `close` still ran: the deadline is an ending, not a crash.
    let mut closed = Value::Null;
    for _ in 0..40 {
        tokio::time::sleep(Duration::from_millis(50)).await;
        closed = audit(&s.runtime, "push-closed:bounded").await;
        if closed != Value::Null {
            break;
        }
    }
    assert_eq!(closed, json!(true), "close did not run at the deadline");
    s.shutdown.cancel();
}

/// A socket's upgrade used to skip boundary validation entirely — the one
/// place on an HTTP surface where C6 ("fail before the world exists") did
/// not reach, and it was the path segment or query that decides what the
/// client is subscribing to. A bad one is a `400` now, and no connection is
/// made.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_socket_upgrade_is_validated_before_the_world() {
    let Some(s) = start().await else { return };
    // `who` is declared `min(1).max(32)`; 40 characters is not.
    let long = "w".repeat(40);
    let refused = tokio_tungstenite::connect_async(format!("{}/push?who={long}", s.ws)).await;
    match refused {
        Ok((_, response)) => panic!("the upgrade succeeded: {}", response.status()),
        Err(tokio_tungstenite::tungstenite::Error::Http(response)) => {
            assert_eq!(response.status(), 400);
            let body =
                String::from_utf8_lossy(response.body().as_deref().unwrap_or(&[])).to_string();
            assert!(body.contains("\"slot\":\"query\""), "{body}");
        }
        Err(e) => panic!("unexpected error: {e}"),
    }
    // And no world ran: the handler's first act is to write its own
    // presence row, and there is none. (A process-wide world counter would
    // not do — the fixture's services and cron create worlds of their own
    // while this test runs.)
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(
        audit(&s.runtime, &format!("push:{long}")).await,
        Value::Null,
        "the handler ran for a request that should not have reached a world"
    );
    s.shutdown.cancel();
}

/// The shape every realtime application needs — a server-push loop in
/// `open` — and the three things that used to be wrong with it, measured by
/// a realtime round on 0.0.10:
///
/// 1. the loop never saw its own client leave (`ctx.signal` stayed clear for
///    5.8 s past the close frame, and the world kept writing to the database);
/// 2. `close` never ran, because the normal end of such a loop is `ctx.send`
///    rejecting with `client_gone` and an `open` that threw skipped it — 717
///    presence rows were left behind over one session;
/// 3. the disconnect was an ERROR line per connection (506 in the minute
///    250 dashboard tabs closed).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_push_loop_sees_its_client_leave_and_its_close_handler_runs() {
    let Some(s) = start().await else { return };
    let (mut ws, response) = tokio_tungstenite::connect_async(format!("{}/push?who=dash", s.ws))
        .await
        .expect("upgrade");
    assert_eq!(response.status(), 101);
    // It is pushing.
    assert_eq!(recv_json(&mut ws).await["i"], 0);
    assert_eq!(recv_json(&mut ws).await["i"], 1);
    assert_eq!(audit(&s.runtime, "push:dash").await, json!("open"));

    // The client goes away without a handshake, the way a killed tab does.
    drop(ws);

    // The loop is bounded at 400 iterations × 50 ms = 20 s, so anything
    // under a second proves the abort reached it rather than the loop
    // running itself out.
    let mut closed = Value::Null;
    for _ in 0..40 {
        tokio::time::sleep(Duration::from_millis(50)).await;
        closed = audit(&s.runtime, "push-closed:dash").await;
        if closed != Value::Null {
            break;
        }
    }
    assert_eq!(closed, json!(true), "close did not run for a push loop");
    let after = audit(&s.runtime, "push:dash").await;
    assert!(
        after
            .as_str()
            .is_some_and(|v| v.starts_with("aborted after")),
        "the loop did not see ctx.signal abort: {after}"
    );
    s.shutdown.cancel();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn socket_state_is_connection_local_and_ends_with_the_connection() {
    let Some(s) = start().await else { return };
    let mut a = connect(&s, "ayu").await;
    let mut b = connect(&s, "budi").await;
    a.send(Message::Text(json!({ "text": "hi" }).to_string().into()))
        .await
        .unwrap();
    a.send(Message::Text(json!({ "text": "again" }).to_string().into()))
        .await
        .unwrap();
    b.send(Message::Text(json!({ "text": "yo" }).to_string().into()))
        .await
        .unwrap();
    assert_eq!(
        recv_json(&mut a).await,
        json!({ "echo": "ayu: hi", "count": 1 })
    );
    assert_eq!(
        recv_json(&mut a).await,
        json!({ "echo": "ayu: again", "count": 2 }),
        "state survives messages"
    );
    assert_eq!(
        recv_json(&mut b).await,
        json!({ "echo": "budi: yo", "count": 1 }),
        "connections do not share state"
    );
    // Contract violation is reported, connection stays open.
    a.send(Message::Text(json!({ "nope": 1 }).to_string().into()))
        .await
        .unwrap();
    let err = recv_json(&mut a).await;
    assert_eq!(err["error"]["code"], "validation_failed");
    a.send(Message::Text(
        json!({ "text": "still here" }).to_string().into(),
    ))
    .await
    .unwrap();
    assert_eq!(recv_json(&mut a).await["count"], 3);
    // Socket-world globals are invisible to finite work.
    let seen: Value = s
        .client
        .get(format!("{}/socket-local", s.base))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(seen["sees"], Value::Null);
    // Application-side close runs the close handler; state is gone with the world.
    a.send(Message::Text(json!({ "text": "bye" }).to_string().into()))
        .await
        .unwrap();
    assert_eq!(recv_json(&mut a).await["count"], 4);
    let close = recv_json(&mut a).await;
    assert_eq!(close["close"], "bye then");
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(audit(&s.runtime, "socket:ayu").await, json!(4));
    // Client-side close.
    b.close(None).await.unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(audit(&s.runtime, "socket:budi").await, json!(1));
    assert_eq!(
        live_connection_worlds(&s.runtime).await,
        0,
        "socket worlds ended with their connections"
    );
    // Non-upgrade request to a socket route.
    let r = s
        .client
        .get(format!("{}/chat", s.base))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 426);
    // Accounting: two upgrades, counted as upgrades and successes — a
    // WebSocket connection is not a 5xx (it was, through 0.0.5).
    let status: Value = s
        .client
        .get(format!("{}/_usai/status", s.base))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(status["http"]["upgrades"], json!(2), "{status}");
    assert_eq!(
        status["http"]["by_workload"]["socket:/chat"]["5xx"],
        json!(0),
        "{status}"
    );
    assert_eq!(
        status["http"]["by_workload"]["socket:/chat"]["2xx"],
        json!(2),
        "{status}"
    );
    finish(s).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn drain_closes_sockets_and_runs_close_handlers() {
    let Some(s) = start().await else { return };
    let mut a = connect(&s, "drainee").await;
    a.send(Message::Text(json!({ "text": "one" }).to_string().into()))
        .await
        .unwrap();
    assert_eq!(recv_json(&mut a).await["count"], 1);
    let rev = s.runtime.active().unwrap();
    let b = s
        .runtime
        .install(Arc::clone(&rev.definition))
        .await
        .unwrap();
    s.runtime.activate(b.id).await.unwrap();
    s.runtime.drain(rev.id).await.unwrap();
    let close = recv_json(&mut a).await;
    assert_eq!(close["close"], "server draining");
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(audit(&s.runtime, "socket:drainee").await, json!(1));
    finish(s).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn idle_sockets_are_closed_by_the_runtime() {
    let Some(s) = start().await else { return };
    // A dedicated host with a short idle timeout.
    let host = HttpHost::new(
        Arc::clone(&s.runtime),
        HttpConfig {
            addr: ([127, 0, 0, 1], 0).into(),
            socket_idle_timeout: Duration::from_millis(300),
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
    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://{addr}/chat?user=idle"))
        .await
        .unwrap();
    let close = recv_json(&mut ws).await;
    assert_eq!(close["close"], "idle timeout");
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(
        audit(&s.runtime, "socket:idle").await,
        json!(0),
        "close handler ran with the connection's state"
    );
    token.cancel();
    finish(s).await;
}
