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
    if !root.join("node_modules/usai").exists() {
        return None;
    }
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let out_dir = std::env::temp_dir().join(format!(
        "usai-conn-test-{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    ));
    let engine = QuickJsEngine::new(QuickJsConfig::default());
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
        |_| None,
    );
    let rev = runtime.install(out.definition).await.unwrap();
    runtime.activate(rev.id).await.unwrap();
    let host = HttpHost::new(
        Arc::clone(&runtime),
        HttpConfig {
            addr: ([127, 0, 0, 1], 0).into(),
            expose_diagnostics: true,
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
