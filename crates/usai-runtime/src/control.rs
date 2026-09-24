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
//! POST   /invoke {kind,name,input|args} run a task / cron tick / command / one queue delivery now, in a fresh world
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

use crate::build::load_artifact_trusted;
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
        RuntimeError::UnknownWorkload(_) => {
            error(StatusCode::NOT_FOUND, "unknown_workload", e.to_string())
        }
        RuntimeError::NoActiveRevision => {
            error(StatusCode::CONFLICT, "no_active_revision", e.to_string())
        }
        RuntimeError::NotActive(..) | RuntimeError::DrainTimeout(..) => {
            error(StatusCode::CONFLICT, "invalid_state", e.to_string())
        }
        RuntimeError::TooManyRevisions { .. } => {
            error(StatusCode::CONFLICT, "too_many_revisions", e.to_string())
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
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct InstallRequest {
    artifact: String,
    /// `true` makes the call **idempotent**: if a revision with this
    /// artifact's identity is already installed, it is returned instead of a
    /// second one being made.
    ///
    /// A control plane retries on timeout, and an install that landed but
    /// whose response was lost used to become a second revision holding a
    /// second compiled image — indistinguishable from the first in every
    /// field, counting against the bound, and orphaned because the deployer
    /// only ever learned the second id. Identity is a content hash, so the
    /// runtime can answer "that is already here" exactly; it does not do so
    /// unasked, because holding the same artifact twice on purpose is a
    /// thing a deployer may want.
    #[serde(default)]
    if_absent: bool,
}

/// `POST /revisions/{id}/activate`. `expected_previous` is the
/// compare-and-swap a machine needs: a control plane retries on timeout, and
/// without it a retry that arrives while the old revision is still
/// `draining` is **indistinguishable from a rollback** — so it silently
/// reverts whatever deployed in between, and answers 200. With it, the call
/// is refused unless the revision it is replacing is still the one the
/// caller saw.
#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct ActivateRequest {
    #[serde(default)]
    expected_previous: Option<u64>,
    /// Allows this runtime to start serving a **different application**.
    /// Refused by default: a deploy template that interpolated the wrong
    /// release directory would otherwise replace one service with another,
    /// with a plain 200 and `/health` reporting `ok` throughout.
    #[serde(default)]
    allow_application_change: bool,
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
        // The world knows which workload wrote these and under which
        // request; a test that had the outcome in its hand used to have to
        // go back to `app.logs()` for either. `fields` is an object here,
        // as it is on every other surface — it is a string inside the
        // world's own record only because that is how it crosses the guest
        // boundary.
        "logs": result.logs.iter().map(|line| json!({
            "level": line.level,
            "message": line.message,
            "fields": line.fields.as_deref()
                .and_then(|f| serde_json::from_str::<Value>(f).ok())
                .unwrap_or(Value::Null),
            "workload": result.workload,
            "world": result.world.to_string(),
            "requestId": result.request_id,
        })).collect::<Vec<_>>(),
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

    /// `rev12` as the runtime prints it, or the bare `12` the JSON carries.
    fn revision_id(segment: &str) -> Option<RevisionId> {
        segment
            .strip_prefix("rev")
            .unwrap_or(segment)
            .parse()
            .ok()
            .map(RevisionId)
    }

    pub async fn handle(self: Arc<Self>, request: Request<Incoming>) -> ControlResponse {
        // A browser can reach a loopback listener. `POST /stop` needs no
        // body, and a form-encoded or `text/plain` POST is a CORS **simple
        // request** — no preflight, no consent — so any page the operator
        // visits could stop the runtime or install an artifact already on
        // the host, and the token-less loopback configuration this surface
        // permits made that unauthenticated. The attacker cannot read the
        // reply and does not need to.
        //
        // Two independent closures, both cheap: a mutating verb must carry
        // `Content-Type: application/json` (which is not a simple request,
        // so it is preflighted and the preflight is refused), and any
        // request carrying `Origin` is refused outright — no browser omits
        // it on a cross-origin request, and no deploy script sends one.
        if request.method() != Method::GET
            && let Some(origin) = request.headers().get(header::ORIGIN)
        {
            return error(
                StatusCode::FORBIDDEN,
                "cross_origin_refused",
                format!(
                    "this is a control surface, not a web API: a request carrying Origin ({}) is refused. A page in a browser must not be able to drive a deployment",
                    origin.to_str().unwrap_or("?")
                ),
            );
        }
        // The three media types a browser may send without a preflight. A
        // control request is JSON (or has no body at all); one of these is
        // a page pretending to be a deployer.
        if matches!(request.method(), &Method::POST)
            && request
                .headers()
                .get(header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .is_some_and(|v| {
                    let v = v.trim_start().to_ascii_lowercase();
                    v.starts_with("application/x-www-form-urlencoded")
                        || v.starts_with("multipart/form-data")
                        || v.starts_with("text/plain")
                })
        {
            return error(
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "unsupported_media_type",
                "control requests are JSON: send `Content-Type: application/json` or no body at all. A form or text/plain POST is a CORS simple request, which a browser may make without asking anyone",
            );
        }
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
                let definition = match load_artifact_trusted(
                    std::path::Path::new(&install.artifact),
                    &self.runtime.config().trusted_signers,
                )
                .await
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
                // A control plane retries on timeout, and an install that
                // landed but whose response was lost used to become a second
                // revision holding a second compiled image — indistinguishable
                // from the first in every field, and counting against the
                // bound of eight. Identity is already a content hash: an
                // `installed` revision with the same one *is* this request's
                // outcome, so return it rather than making another.
                let identity = definition.identity().to_owned();
                if let Some(existing) =
                    install
                        .if_absent
                        .then(|| {
                            self.runtime.status().revisions.into_iter().find(|r| {
                                r.state == RevisionState::Installed && r.identity == identity
                            })
                        })
                        .flatten()
                {
                    return reply(
                        StatusCode::OK,
                        json!({
                            "id": existing.id,
                            "identity": existing.identity,
                            "application": existing.application,
                            "state": existing.state,
                            "installed": false,
                        }),
                    );
                }
                match self.runtime.install(definition).await {
                    Ok(rev) => reply(
                        StatusCode::CREATED,
                        json!({ "id": rev.id, "identity": rev.definition.identity(), "application": rev.definition.name(), "state": rev.state(), "installed": true }),
                    ),
                    Err(e) => runtime_error(e),
                }
            }
            (&Method::POST, ["revisions", id, "activate"]) => {
                let Some(id) = Self::revision_id(id) else {
                    return error(StatusCode::NOT_FOUND, "unknown_revision", "bad revision id");
                };
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
                let wanted: ActivateRequest = if body.is_empty() {
                    ActivateRequest::default()
                } else {
                    match serde_json::from_slice(&body) {
                        Ok(a) => a,
                        Err(e) => {
                            return error(
                                StatusCode::BAD_REQUEST,
                                "invalid_request",
                                e.to_string(),
                            );
                        }
                    }
                };
                let previous = self.runtime.active().ok().map(|r| r.id);
                if let Some(expected) = wanted.expected_previous
                    && previous.map(|p| p.0) != Some(expected)
                {
                    return error(
                        StatusCode::CONFLICT,
                        "active_revision_moved",
                        format!(
                            "expected rev{expected} to be active, found {}. Another deploy landed since you looked: activating now would revert it",
                            previous
                                .map(|p| p.to_string())
                                .unwrap_or_else(|| "none".into())
                        ),
                    );
                }
                // A runtime serving one application must not silently become
                // another because a deploy template interpolated the wrong
                // release directory. The runtime knows both names.
                if !wanted.allow_application_change
                    && let (Ok(active), Ok(target)) =
                        (self.runtime.active(), self.runtime.revision(id))
                    && active.definition.name() != target.definition.name()
                {
                    return error(
                        StatusCode::CONFLICT,
                        "different_application",
                        format!(
                            "this runtime is serving {:?}; rev{} is {:?}. Activating it would replace one application with another — start a second runtime, or pass {{\"allowApplicationChange\": true}} if you mean it",
                            active.definition.name(),
                            id.0,
                            target.definition.name()
                        ),
                    );
                }
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
                // Draining the only revision there is takes the runtime out
                // of service with nothing to put back: every request is 503
                // `no_active_revision`, and the only way back is a fresh
                // install with an artifact path this process no longer
                // remembers. One call, a millisecond, no confirmation. That
                // is a maintenance window for someone who meant it, and an
                // outage for a deployer who reached for `drain` after
                // `activate` (which already drained the old revision itself).
                if self.runtime.active().is_ok_and(|active| active.id == id)
                    && self.runtime.status().revisions.len() == 1
                {
                    return error(
                        StatusCode::CONFLICT,
                        "would_stop_serving",
                        format!(
                            "rev{} is the only revision: draining it leaves nothing to serve and nothing to activate. `activate` drains the revision it replaces by itself — you do not need this in a deploy. For a deliberate maintenance window, install the next revision first",
                            id.0
                        ),
                    );
                }
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
                    "queue" => {
                        self.runtime
                            .run_queue_message(&invoke.name, invoke.input)
                            .await
                    }
                    other => {
                        return error(
                            StatusCode::BAD_REQUEST,
                            "invalid_kind",
                            format!(
                                "cannot invoke kind {other}; use task, cron, command, or queue"
                            ),
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
