//! WebSocket connections (`GOAL.md` §20): one world per connection,
//! connection-local mutable state, state ends with the connection.
//!
//! Frames arrive as host completions into the same world (`socket.recv`);
//! sends are host operations (`socket.send`). A graceful stop closes the
//! socket and lets the handler's `close` run.

use std::sync::Arc;

use futures_util::{SinkExt, StreamExt};
use hyper::upgrade::Upgraded;
use hyper_util::rt::TokioIo;
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::{Mutex, mpsc};
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::protocol::Role;
use tokio_util::sync::CancellationToken;

use crate::host_ops::{OpContext, OpFuture, OpHandler, OpOutcome};

/// One event the guest receives from `socket.recv`.
#[derive(Clone, Debug, serde::Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum Inbound {
    Text { data: String },
    Binary { base64: String },
    Close { code: Option<u16>, reason: String },
}

pub struct SocketLink {
    inbound: Mutex<mpsc::Receiver<Inbound>>,
    outbound: mpsc::Sender<Message>,
    pub stop: CancellationToken,
    /// Fired once when the handler accepts the connection (`socket.accept`,
    /// or implicitly on its first `recv`/`send`), carrying the subprotocol
    /// to echo in the 101, if any. Until then the HTTP response is not
    /// sent: an auth resolver that refuses answers a 401, not a 101 followed
    /// by a bare close.
    accepted: std::sync::Mutex<Option<tokio::sync::oneshot::Sender<Option<String>>>>,
}

impl SocketLink {
    pub fn new(
        inbound: mpsc::Receiver<Inbound>,
        outbound: mpsc::Sender<Message>,
    ) -> (Arc<Self>, tokio::sync::oneshot::Receiver<Option<String>>) {
        let (tx, rx) = tokio::sync::oneshot::channel();
        (
            Arc::new(Self {
                inbound: Mutex::new(inbound),
                outbound,
                stop: CancellationToken::new(),
                accepted: std::sync::Mutex::new(Some(tx)),
            }),
            rx,
        )
    }

    /// Marks the connection accepted; a second call is a no-op.
    pub fn accept(&self, protocol: Option<String>) {
        if let Some(tx) = self.accepted.lock().expect("accepted poisoned").take() {
            let _ = tx.send(protocol);
        }
    }
}

/// Pumps frames between the upgraded connection and the world's link.
/// Runs for the connection's lifetime; ends when either side closes.
/// `stop` is this connection's own token; `draining` is the revision-wide
/// one it descends from. Both look identical from here once cancelled, which
/// is how every socket ending at its **own** declared deadline came to close
/// `1012 server draining` on a runtime that was serving normally — telling
/// every client the server was restarting, and sending whoever saw it to
/// look at deploys. The parent is what tells the two apart.
pub async fn pump(
    upgraded: Upgraded,
    inbound: mpsc::Sender<Inbound>,
    mut outbound: mpsc::Receiver<Message>,
    stop: CancellationToken,
    draining: CancellationToken,
    idle_timeout: std::time::Duration,
) {
    let ws = WebSocketStream::from_raw_socket(TokioIo::new(upgraded), Role::Server, None).await;
    let (mut sink, mut source) = ws.split();
    loop {
        let idle = tokio::time::sleep(idle_timeout);
        tokio::select! {
            _ = idle => {
                let _ = sink.send(Message::Close(Some(tokio_tungstenite::tungstenite::protocol::CloseFrame {
                    code: tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode::Policy,
                    reason: "idle timeout".into(),
                }))).await;
                let _ = inbound.send(Inbound::Close { code: Some(1008), reason: "idle timeout".into() }).await;
                break;
            }
            incoming = source.next() => match incoming {
                Some(Ok(Message::Text(text))) => {
                    if inbound.send(Inbound::Text { data: text.to_string() }).await.is_err() { break; }
                }
                Some(Ok(Message::Binary(bytes))) => {
                    use base64::Engine as _;
                    if inbound.send(Inbound::Binary { base64: base64::engine::general_purpose::STANDARD.encode(&bytes) }).await.is_err() { break; }
                }
                Some(Ok(Message::Close(frame))) => {
                    let (code, reason) = frame.map(|f| (Some(u16::from(f.code)), f.reason.to_string())).unwrap_or((None, String::new()));
                    let _ = inbound.send(Inbound::Close { code, reason }).await;
                    break;
                }
                Some(Ok(Message::Ping(payload))) => { let _ = sink.send(Message::Pong(payload)).await; }
                Some(Ok(_)) => {}
                Some(Err(e)) => {
                    let _ = inbound.send(Inbound::Close { code: None, reason: e.to_string() }).await;
                    break;
                }
                None => {
                    let _ = inbound.send(Inbound::Close { code: None, reason: "connection closed".into() }).await;
                    break;
                }
            },
            outgoing = outbound.recv() => match outgoing {
                Some(message) => {
                    let closing = matches!(message, Message::Close(_));
                    if sink.send(message).await.is_err() || closing { break; }
                }
                None => break,
            },
            _ = stop.cancelled() => {
                use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
                // 1012 means "the server is restarting, come back" and
                // clients act on it. It is the truth only when the revision
                // really is draining; when this one connection ended — its
                // declared deadline, its world finishing — the honest frame
                // is 1001, the endpoint going away.
                let (code, reason) = if draining.is_cancelled() {
                    (CloseCode::Restart, "server draining")
                } else {
                    (CloseCode::Away, "server closed this connection")
                };
                let _ = sink.send(Message::Close(Some(tokio_tungstenite::tungstenite::protocol::CloseFrame {
                    code,
                    reason: reason.into(),
                }))).await;
                let _ = inbound.send(Inbound::Close { code: Some(u16::from(code)), reason: reason.into() }).await;
                break;
            }
        }
    }
    let _ = sink.close().await;
    // However this connection ended — a close frame, an error, the client
    // vanishing, the idle timeout — the world that serves it is done. `stop`
    // is this connection's own token (a child of the revision's drain), so
    // cancelling it aborts `ctx.signal` and returns `ctx.sleep` in that one
    // world, which is how a handler looping in `open` learns its client left.
    stop.cancel();
}

fn link(ctx: &OpContext) -> Result<Arc<SocketLink>, OpOutcome> {
    ctx.attachment
        .as_ref()
        .and_then(|a| Arc::clone(a).downcast::<SocketLink>().ok())
        .ok_or_else(|| OpOutcome::err("not_a_socket", 500, "this workload is not a socket"))
}

/// `socket.accept`: the handler (after its auth resolver) lets the upgrade
/// complete; `{ "protocol": "bearer" }` names the subprotocol to echo.
pub struct AcceptHandler;

impl OpHandler for AcceptHandler {
    fn start(&self, ctx: OpContext, payload: String) -> Result<OpFuture, OpOutcome> {
        let link = link(&ctx)?;
        let protocol = serde_json::from_str::<Value>(&payload)
            .ok()
            .and_then(|v| v.get("protocol").and_then(Value::as_str).map(str::to_owned));
        link.accept(protocol);
        Ok(Box::pin(async move { OpOutcome::ok(&json!(true)) }))
    }
}

/// `socket.recv`: resolves with the next inbound event.
pub struct RecvHandler;

impl OpHandler for RecvHandler {
    fn start(&self, ctx: OpContext, _payload: String) -> Result<OpFuture, OpOutcome> {
        let link = link(&ctx)?;
        // A bundle that predates `socket.accept` accepts by using the socket.
        link.accept(None);
        let cancel = ctx.cancel.clone();
        Ok(Box::pin(async move {
            let mut inbound = link.inbound.lock().await;
            tokio::select! {
                next = inbound.recv() => match next {
                    Some(event) => OpOutcome::ok(&serde_json::to_value(event).unwrap_or(Value::Null)),
                    None => OpOutcome::ok(&json!({ "type": "close", "code": null, "reason": "connection closed" })),
                },
                _ = cancel.cancelled() => OpOutcome::ok(&json!({ "type": "close", "code": null, "reason": "cancelled" })),
            }
        }))
    }
}

#[derive(Deserialize)]
struct SendRequest {
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    base64: Option<String>,
}

/// `socket.send`: one outbound frame.
pub struct SendHandler;

impl OpHandler for SendHandler {
    fn start(&self, ctx: OpContext, payload: String) -> Result<OpFuture, OpOutcome> {
        let link = link(&ctx)?;
        link.accept(None);
        let request: SendRequest = serde_json::from_str(&payload)
            .map_err(|e| OpOutcome::err("invalid_socket_send", 500, e.to_string()))?;
        let message = if let Some(text) = request.text {
            Message::Text(text.into())
        } else if let Some(b64) = request.base64 {
            use base64::Engine as _;
            Message::Binary(
                base64::engine::general_purpose::STANDARD
                    .decode(b64)
                    .unwrap_or_default()
                    .into(),
            )
        } else {
            return Err(OpOutcome::err(
                "invalid_socket_send",
                500,
                "text or base64 required",
            ));
        };
        Ok(Box::pin(async move {
            match link.outbound.send(message).await {
                Ok(()) => OpOutcome::ok(&json!(true)),
                Err(_) => OpOutcome::err("client_gone", 499, "the connection is closed"),
            }
        }))
    }
}

/// `socket.close`: close from the application side.
pub struct CloseHandler;

impl OpHandler for CloseHandler {
    fn start(&self, ctx: OpContext, payload: String) -> Result<OpFuture, OpOutcome> {
        let link = link(&ctx)?;
        let reason: String = serde_json::from_str::<Value>(&payload)
            .ok()
            .and_then(|v| v.get("reason").and_then(Value::as_str).map(str::to_owned))
            .unwrap_or_default();
        Ok(Box::pin(async move {
            let _ = link
                .outbound
                .send(Message::Close(Some(tokio_tungstenite::tungstenite::protocol::CloseFrame {
                    code: tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode::Normal,
                    reason: reason.into(),
                })))
                .await;
            OpOutcome::ok(&json!(true))
        }))
    }
}
