//! Streaming HTTP responses (`GOAL.md` §21): the world lives until the
//! handler returns, not until headers are sent. `headers sent != work
//! complete`.

use std::sync::{Arc, Mutex};

use bytes::Bytes;
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;

use crate::host_ops::{OpContext, OpFuture, OpHandler, OpOutcome};

/// What the guest committed before the first chunk.
pub struct StreamHead {
    pub status: u16,
    pub headers: Vec<(String, String)>,
}

/// Attached to a stream world. The pipeline holds the receiving ends.
pub struct StreamSink {
    head: Mutex<Option<oneshot::Sender<StreamHead>>>,
    body: mpsc::Sender<Bytes>,
    default_content_type: String,
}

impl StreamSink {
    pub fn new(
        default_content_type: &str,
    ) -> (
        Arc<Self>,
        oneshot::Receiver<StreamHead>,
        mpsc::Receiver<Bytes>,
    ) {
        let (head_tx, head_rx) = oneshot::channel();
        let (body_tx, body_rx) = mpsc::channel(64);
        (
            Arc::new(Self {
                head: Mutex::new(Some(head_tx)),
                body: body_tx,
                default_content_type: default_content_type.to_owned(),
            }),
            head_rx,
            body_rx,
        )
    }

    fn commit_head(&self, status: u16, headers: Vec<(String, String)>) {
        if let Some(tx) = self.head.lock().expect("head poisoned").take() {
            let mut headers = headers;
            if !headers
                .iter()
                .any(|(k, _)| k.eq_ignore_ascii_case("content-type"))
            {
                headers.push(("content-type".into(), self.default_content_type.clone()));
            }
            let _ = tx.send(StreamHead { status, headers });
        }
    }
}

#[derive(Deserialize)]
struct StartRequest {
    #[serde(default = "ok")]
    status: u16,
    #[serde(default)]
    headers: std::collections::BTreeMap<String, String>,
}

fn ok() -> u16 {
    200
}

#[derive(Deserialize)]
struct SendRequest {
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    base64: Option<String>,
}

fn sink(ctx: &OpContext) -> Result<Arc<StreamSink>, OpOutcome> {
    ctx.attachment
        .as_ref()
        .and_then(|a| Arc::clone(a).downcast::<StreamSink>().ok())
        .ok_or_else(|| OpOutcome::err("not_a_stream", 500, "this workload is not a stream"))
}

/// `stream.start`: commit status/headers before the first chunk.
pub struct StartHandler;

impl OpHandler for StartHandler {
    fn start(&self, ctx: OpContext, payload: String) -> Result<OpFuture, OpOutcome> {
        let sink = sink(&ctx)?;
        let request: StartRequest = serde_json::from_str(&payload)
            .map_err(|e| OpOutcome::err("invalid_stream_start", 500, e.to_string()))?;
        sink.commit_head(request.status, request.headers.into_iter().collect());
        Ok(Box::pin(async { OpOutcome::ok(&Value::Null) }))
    }
}

/// `stream.send`: one chunk. Commits a 200 head on first use.
pub struct SendHandler;

impl OpHandler for SendHandler {
    fn start(&self, ctx: OpContext, payload: String) -> Result<OpFuture, OpOutcome> {
        let sink = sink(&ctx)?;
        let request: SendRequest = serde_json::from_str(&payload)
            .map_err(|e| OpOutcome::err("invalid_stream_send", 500, e.to_string()))?;
        let bytes = if let Some(text) = request.text {
            Bytes::from(text)
        } else if let Some(b64) = request.base64 {
            use base64::Engine as _;
            Bytes::from(
                base64::engine::general_purpose::STANDARD
                    .decode(b64)
                    .unwrap_or_default(),
            )
        } else {
            Bytes::new()
        };
        sink.commit_head(200, vec![]);
        let cancel: CancellationToken = ctx.cancel.clone();
        Ok(Box::pin(async move {
            tokio::select! {
                sent = sink.body.send(bytes) => match sent {
                    Ok(()) => OpOutcome::ok(&json!(true)),
                    Err(_) => OpOutcome::err("client_gone", 499, "the client is no longer reading the stream"),
                },
                _ = cancel.cancelled() => OpOutcome::err("cancelled", 499, "stream cancelled"),
            }
        }))
    }
}
