//! The local control surface (`GOAL.md` D15): what an orchestrator needs
//! to install, activate, inspect, drain, and remove revisions, and to stop
//! the runtime. Generic JSON over HTTP on a separate listener; Sakala is
//! one client of it, never a dependency.
//!
//! ```text
//! GET    /health                      liveness + active revision + gauges
//! GET    /status                      full RuntimeStatus
//! GET    /revisions                   revisions with state
//! POST   /revisions {artifact}        install from an artifact directory
//! POST   /revisions/{id}/activate     activate (previous starts draining)
//! POST   /revisions/{id}/drain        drain and retire
//! DELETE /revisions/{id}              remove an installed (never active) revision
//! POST   /stop                        graceful shutdown of the runtime
//! POST   /invoke {kind,name,input|args} run a task / cron tick / command now, in a fresh world
//! ```
//!
//! Authentication: a bearer token from `USAI_CONTROL_TOKEN`. Binding to a
//! non-loopback address without a token is refused.

use std::net::SocketAddr;
use std::sync::Arc;

use bytes::Bytes;
use http::{Method, Request, Response, StatusCode, header};
use http_body_util::{BodyExt, Full, Limited};
use hyper::body::Incoming;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper_util::rt::TokioIo;
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

use crate::build::load_artifact;
use crate::runtime::{RevisionId, RevisionState, Runtime, RuntimeError};

#[derive(Clone, Debug)]
pub struct ControlConfig {
    pub addr: SocketAddr,
    pub token: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum ControlError {
    #[error("refusing to bind the control surface to {0} without USAI_CONTROL_TOKEN")]
    TokenRequired(SocketAddr),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub struct ControlHost {
    runtime: Arc<Runtime>,
    config: ControlConfig,
    /// Fires when a client asked the runtime to stop.
    pub stop_requested: CancellationToken,
}

type ControlResponse = Response<Full<Bytes>>;

fn reply(status: StatusCode, body: Value) -> ControlResponse {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Full::new(Bytes::from(
            serde_json::to_vec(&body).unwrap_or_default(),
        )))
        .expect("static response")
}

fn error(status: StatusCode, code: &str, message: impl Into<String>) -> ControlResponse {
    reply(
        status,
        json!({ "error": { "code": code, "message": message.into() } }),
    )
}

fn runtime_error(e: RuntimeError) -> ControlResponse {
    match e {
        RuntimeError::UnknownRevision(_) => {
            error(StatusCode::NOT_FOUND, "unknown_revision", e.to_string())
        }
        RuntimeError::NotActive(..) | RuntimeError::DrainTimeout(..) => {
            error(StatusCode::CONFLICT, "invalid_state", e.to_string())
        }
        RuntimeError::MissingEnv(_)
        | RuntimeError::InvalidEnv(_)
        | RuntimeError::InvalidDefinition(_)
        | RuntimeError::Resource(_) => error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "activation_failed",
            e.to_string(),
        ),
        other => error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "runtime_error",
            other.to_string(),
        ),
    }
}

#[derive(Deserialize)]
struct InstallRequest {
    artifact: String,
}

#[derive(Deserialize)]
struct InvokeRequest {
    kind: String,
    name: String,
    #[serde(default)]
    input: Value,
    #[serde(default)]
    args: Vec<String>,
}

fn work_result_json(result: &crate::world::WorkResult) -> Value {
    let (ok, value, error) = match &result.outcome {
        Some(Ok(v)) => (
            true,
            v.get("value").cloned().unwrap_or(v.clone()),
            Value::Null,
        ),
        Some(Err(e)) => (
            false,
            Value::Null,
            json!({ "name": e.name, "message": e.message, "usai": e.usai }),
        ),
        None => (false, Value::Null, Value::Null),
    };
    json!({
        "ok": ok,
        "value": value,
        "error": error,
        "termination": result.termination,
        "world": result.world,
        "durationMs": result.duration.as_millis() as u64,
        "violations": result.violations,
        "logs": result.logs,
        "children": result.children,
    })
}

impl ControlHost {
    pub fn new(runtime: Arc<Runtime>, config: ControlConfig) -> Result<Arc<Self>, ControlError> {
        if !config.addr.ip().is_loopback() && config.token.is_none() {
            return Err(ControlError::TokenRequired(config.addr));
        }
        Ok(Arc::new(Self {
            runtime,
            config,
            stop_requested: CancellationToken::new(),
        }))
    }

    fn authorized(&self, request: &Request<Incoming>) -> bool {
        match &self.config.token {
            None => true,
            Some(token) => request
                .headers()
                .get(header::AUTHORIZATION)
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.strip_prefix("Bearer "))
                .is_some_and(|presented| presented.trim() == token),
        }
    }

    fn revision_id(segment: &str) -> Option<RevisionId> {
        segment
            .strip_prefix("rev")
            .and_then(|n| n.parse().ok())
            .map(RevisionId)
    }

    pub async fn handle(self: Arc<Self>, request: Request<Incoming>) -> ControlResponse {
        if !self.authorized(&request) {
            return error(
                StatusCode::UNAUTHORIZED,
                "unauthorized",
                "missing or invalid control token",
            );
        }
        let method = request.method().clone();
        let path = request.uri().path().to_owned();
        let segments: Vec<&str> = path.trim_matches('/').split('/').collect();
        match (&method, segments.as_slice()) {
            (&Method::GET, ["health"]) => {
                let status = self.runtime.status();
                let active = status
                    .revisions
                    .iter()
                    .find(|r| r.state == RevisionState::Active);
                reply(
                    StatusCode::OK,
                    json!({
                        "ok": active.is_some(),
                        "active": active.map(|r| json!({ "id": r.id, "identity": r.identity, "application": r.application })),
                        "gauges": status.gauges,
                    }),
                )
            }
            (&Method::GET, ["status"]) => reply(
                StatusCode::OK,
                serde_json::to_value(self.runtime.status()).unwrap_or(Value::Null),
            ),
            (&Method::GET, ["revisions"]) => reply(
                StatusCode::OK,
                json!({ "revisions": self.runtime.status().revisions }),
            ),
            (&Method::POST, ["revisions"]) => {
                let body = match Limited::new(request.into_body(), 64 * 1024).collect().await {
                    Ok(b) => b.to_bytes(),
                    Err(_) => {
                        return error(
                            StatusCode::PAYLOAD_TOO_LARGE,
                            "payload_too_large",
                            "request body too large",
                        );
                    }
                };
                let install: InstallRequest = match serde_json::from_slice(&body) {
                    Ok(i) => i,
                    Err(e) => {
                        return error(StatusCode::BAD_REQUEST, "invalid_request", e.to_string());
                    }
                };
                let definition = match load_artifact(std::path::Path::new(&install.artifact)).await
                {
                    Ok(d) => d,
                    Err(e) => {
                        return error(
                            StatusCode::UNPROCESSABLE_ENTITY,
                            "invalid_artifact",
                            e.to_string(),
                        );
                    }
                };
                match self.runtime.install(definition).await {
                    Ok(rev) => reply(
                        StatusCode::CREATED,
                        json!({ "id": rev.id, "identity": rev.definition.identity(), "application": rev.definition.name(), "state": rev.state() }),
                    ),
                    Err(e) => runtime_error(e),
                }
            }
            (&Method::POST, ["revisions", id, "activate"]) => {
                let Some(id) = Self::revision_id(id) else {
                    return error(StatusCode::NOT_FOUND, "unknown_revision", "bad revision id");
                };
                let previous = self.runtime.active().ok().map(|r| r.id);
                match self.runtime.activate(id).await {
                    Ok(rev) => reply(
                        StatusCode::OK,
                        json!({ "id": rev.id, "state": rev.state(), "previous": previous.filter(|p| *p != id) }),
                    ),
                    Err(e) => runtime_error(e),
                }
            }
            (&Method::POST, ["revisions", id, "drain"]) => {
                let Some(id) = Self::revision_id(id) else {
                    return error(StatusCode::NOT_FOUND, "unknown_revision", "bad revision id");
                };
                match self.runtime.drain(id).await {
                    Ok(()) => reply(
                        StatusCode::OK,
                        json!({ "id": id, "state": RevisionState::Retired }),
                    ),
                    Err(e) => runtime_error(e),
                }
            }
            (&Method::DELETE, ["revisions", id]) => {
                let Some(id) = Self::revision_id(id) else {
                    return error(StatusCode::NOT_FOUND, "unknown_revision", "bad revision id");
                };
                match self.runtime.remove(id) {
                    Ok(()) => reply(StatusCode::OK, json!({ "id": id, "removed": true })),
                    Err(e) => runtime_error(e),
                }
            }
            (&Method::POST, ["invoke"]) => {
                let body = match Limited::new(request.into_body(), 1024 * 1024)
                    .collect()
                    .await
                {
                    Ok(b) => b.to_bytes(),
                    Err(_) => {
                        return error(
                            StatusCode::PAYLOAD_TOO_LARGE,
                            "payload_too_large",
                            "request body too large",
                        );
                    }
                };
                let invoke: InvokeRequest = match serde_json::from_slice(&body) {
                    Ok(i) => i,
                    Err(e) => {
                        return error(StatusCode::BAD_REQUEST, "invalid_request", e.to_string());
                    }
                };
                let result = match invoke.kind.as_str() {
                    "task" => self.runtime.run_task(&invoke.name, invoke.input).await,
                    "cron" => self.runtime.run_cron(&invoke.name).await,
                    "command" => self.runtime.run_command(&invoke.name, invoke.args).await,
                    other => {
                        return error(
                            StatusCode::BAD_REQUEST,
                            "invalid_kind",
                            format!("cannot invoke kind {other}; use task, cron, or command"),
                        );
                    }
                };
                match result {
                    Ok(r) => reply(StatusCode::OK, work_result_json(&r)),
                    Err(e) => runtime_error(e),
                }
            }
            (&Method::POST, ["stop"]) => {
                self.stop_requested.cancel();
                reply(StatusCode::ACCEPTED, json!({ "stopping": true }))
            }
            _ => error(
                StatusCode::NOT_FOUND,
                "unknown_route",
                format!("{method} {path}"),
            ),
        }
    }
}

/// Serves the control surface until `shutdown` fires.
pub async fn serve(
    host: Arc<ControlHost>,
    shutdown: CancellationToken,
    on_bound: impl FnOnce(SocketAddr),
) -> std::io::Result<()> {
    let listener = TcpListener::bind(host.config.addr).await?;
    on_bound(listener.local_addr()?);
    loop {
        let (stream, _) = tokio::select! {
            accepted = listener.accept() => match accepted {
                Ok(pair) => pair,
                Err(_) => continue,
            },
            _ = shutdown.cancelled() => return Ok(()),
        };
        let host = Arc::clone(&host);
        tokio::spawn(async move {
            let _ = http1::Builder::new()
                .serve_connection(
                    TokioIo::new(stream),
                    service_fn(move |request| {
                        let host = Arc::clone(&host);
                        async move { Ok::<_, std::convert::Infallible>(host.handle(request).await) }
                    }),
                )
                .await;
        });
    }
}
