//! The request pipeline. One `HttpHost` per runtime; it caches the compiled
//! routing state for the active revision and rebuilds it only when the
//! active revision changes.

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::sync::{Arc, RwLock};

use base64::Engine as _;
use bytes::Bytes;
use http::{HeaderMap, HeaderValue, Method, Request, Response, StatusCode, header};
use http_body_util::combinators::BoxBody;
use http_body_util::{BodyExt, Full, Limited, StreamBody};
use hyper::body::Frame;
use hyper::body::Incoming;
use serde::Deserialize;
use serde_json::{Value, json};
use std::convert::Infallible;
use tokio_util::sync::CancellationToken;

use super::router::{CompiledRevision, Route, RouteKind, SlotValidators};
use super::socket::{Inbound, SocketLink};
use super::stream::StreamSink;
use crate::engine::GuestError;
use crate::runtime::ExecuteOptions;
use crate::runtime::{Runtime, RuntimeError};
use crate::world::{Termination, WorkResult};

#[derive(Clone, Debug)]
pub struct HttpConfig {
    pub addr: SocketAddr,
    pub max_body_bytes: usize,
    /// Include diagnostic detail (violations, fault text) in responses.
    /// Development only; never enable in production.
    pub expose_diagnostics: bool,
    /// Serve `/_usai/openapi.json` and `/_usai/docs` from the active
    /// definition.
    pub serve_docs: bool,
    /// Serve `/_usai/status` (JSON) and `/_usai/metrics` (Prometheus text).
    pub serve_status: bool,
    /// A WebSocket with no frames in either direction for this long is
    /// closed (1008) so a silent client cannot hold a world forever.
    pub socket_idle_timeout: std::time::Duration,
    /// When set, `/_usai/status`, `/_usai/metrics` and `/_usai/openapi.json`
    /// require `Authorization: Bearer <token>` on either listener;
    /// `/_usai/live` and `/_usai/ready` stay open (probes rarely carry
    /// headers, and they reveal little), and the reference's HTML shell
    /// stays open (it is static and asks the operator for the token before
    /// fetching the document). What these surfaces show — memory, per-route
    /// counters, the revision identity, the full OpenAPI profile with
    /// environment names and schedules — is operator information.
    pub status_token: Option<String>,
    /// Surfaces switched off by name — `status`, `metrics`, `docs` (the
    /// reference and its OpenAPI document), `live`, `ready` — on both
    /// listeners; a switched-off surface answers 404 like any unknown
    /// `/_usai/` path. Production rarely wants all of them.
    pub surfaces_off: Vec<String>,
    /// Whether a bound resource that fails its probe makes `/_usai/ready`
    /// answer 503 (the default) or merely report it while the replica keeps
    /// asking for traffic (`USAI_READY_REQUIRES_RESOURCES=0`).
    ///
    /// It is a deployment decision, not a runtime one. A proxy removes an
    /// unready upstream from rotation, so with the default a database
    /// outage takes out *every* route on every replica that shares that
    /// database — including routes that never touch it. With it off, those
    /// routes keep serving and the database-backed ones answer 503 on their
    /// own. Draining is unaffected either way: a draining replica always
    /// fails readiness, which is what the rolling restart depends on.
    pub ready_requires_resources: bool,
}

impl Default for HttpConfig {
    fn default() -> Self {
        Self {
            addr: ([127, 0, 0, 1], 3000).into(),
            max_body_bytes: 1024 * 1024,
            expose_diagnostics: false,
            serve_docs: false,
            serve_status: false,
            socket_idle_timeout: std::time::Duration::from_secs(300),
            status_token: None,
            surfaces_off: Vec::new(),
            ready_requires_resources: true,
        }
    }
}

pub struct HttpHost {
    runtime: Arc<Runtime>,
    config: HttpConfig,
    compiled: RwLock<Option<Arc<CompiledRevision>>>,
    pub stats: crate::observability::HttpStats,
    /// Set when the process has been told to stop and has not yet closed
    /// its listener: readiness fails and every response asks the peer to
    /// close, so a proxy stops routing here and stops reusing its idle
    /// connections *before* the listener goes away. Without this window a
    /// request written onto a keep-alive connection at the instant the
    /// server closes it is a 502 at the proxy (measured on the two-replica
    /// rolling restart: one per restart).
    draining: std::sync::atomic::AtomicBool,
}

/// A response decided before (or instead of) application work.
struct Reply {
    status: StatusCode,
    body: Value,
    /// Decided before any world existed (routing, validation, admission).
    before_world: bool,
    /// The workload this refusal belongs to, once routing named one.
    workload: Option<String>,
    /// Extra response headers (`Allow` on a 405).
    headers: Vec<(&'static str, String)>,
}

impl Reply {
    fn error(status: StatusCode, code: &str, message: impl Into<String>) -> Self {
        Self {
            status,
            body: json!({ "error": { "code": code, "message": message.into() } }),
            before_world: true,
            workload: None,
            headers: Vec::new(),
        }
    }

    fn with_header(mut self, name: &'static str, value: String) -> Self {
        self.headers.push((name, value));
        self
    }

    /// A failure after admission: a world existed or was being created.
    fn after_world(status: StatusCode, code: &str, message: impl Into<String>) -> Self {
        Self {
            before_world: false,
            ..Self::error(status, code, message)
        }
    }

    fn with_details(mut self, details: Value) -> Self {
        self.body["error"]["details"] = details;
        self
    }
}

/// A response header the guest set: one value, or several for a header that
/// repeats (`set-cookie`).
#[derive(Deserialize)]
#[serde(untagged)]
pub(crate) enum HeaderValues {
    One(String),
    Many(Vec<String>),
}

impl HeaderValues {
    fn iter(&self) -> impl Iterator<Item = &str> {
        match self {
            HeaderValues::One(v) => std::slice::from_ref(v).iter().map(String::as_str),
            HeaderValues::Many(vs) => vs.iter().map(String::as_str),
        }
    }
}

#[derive(Deserialize)]
pub(crate) struct GuestHttpOutput {
    status: u16,
    #[serde(default)]
    headers: BTreeMap<String, HeaderValues>,
    #[serde(default)]
    json: Option<Value>,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    base64: Option<String>,
}

pub type HttpResponse = Response<BoxBody<Bytes, Infallible>>;

/// The host-side phases of one request, for the attribution harness
/// (`USAI_PROFILE=1`): route → decode → validate → admit → execute → encode.
/// Off, it holds an empty `Vec` (no allocation) and every `lap` is a no-op.
struct Stopwatch {
    on: bool,
    last: std::time::Instant,
    phases: Vec<(&'static str, f64)>,
}

impl Stopwatch {
    fn start() -> Self {
        Self {
            on: crate::engine::profiling(),
            last: std::time::Instant::now(),
            phases: Vec::new(),
        }
    }

    fn lap(&mut self, phase: &'static str) {
        if self.on {
            let now = std::time::Instant::now();
            self.phases
                .push((phase, (now - self.last).as_secs_f64() * 1000.0));
            self.last = now;
        }
    }
}

/// `x-usai-profile: http.route=0.012,http.decode=0.030,…,guest.handler=0.201`
/// — the whole invoice for one request, host and guest phases, only when
/// profiling is on. The harness (`tests/profile_matrix.rs`, http mode)
/// reads it; nobody else should.
fn profile_header(host: &[(&'static str, f64)], world: &[(String, f64)]) -> Option<HeaderValue> {
    let mut text = String::new();
    for (k, v) in host {
        text.push_str(&format!("http.{k}={v:.4},"));
    }
    for (k, v) in world {
        text.push_str(&format!("{k}={v:.4},"));
    }
    text.pop();
    HeaderValue::from_str(&text).ok()
}

fn json_response(status: StatusCode, body: &Value) -> HttpResponse {
    let bytes = serde_json::to_vec(body).unwrap_or_else(|_| b"{}".to_vec());
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Full::new(Bytes::from(bytes)).boxed())
        .expect("static response")
}

/// Streaming body whose drop (client gone) cancels the world.
struct WorldBody {
    receiver: tokio::sync::mpsc::Receiver<Bytes>,
    _guard: tokio_util::sync::DropGuard,
}

impl futures_util::Stream for WorldBody {
    type Item = Result<Frame<Bytes>, Infallible>;
    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        self.receiver
            .poll_recv(cx)
            .map(|item| item.map(|bytes| Ok(Frame::data(bytes))))
    }
}

const REQUEST_ID: http::HeaderName = http::HeaderName::from_static("x-request-id");

/// The client's `x-request-id` when it is short and printable (a proxy's
/// trace id, a test's marker), else a fresh UUID. Never the client's bytes
/// verbatim into a log line.
fn request_id_for(headers: &HeaderMap) -> String {
    headers
        .get(REQUEST_ID)
        .and_then(|v| v.to_str().ok())
        .filter(|s| {
            !s.is_empty()
                && s.len() <= 128
                && s.bytes()
                    .all(|b| b.is_ascii_graphic() && b != b'"' && b != b'\\')
        })
        .map(str::to_owned)
        .unwrap_or_else(|| {
            let mut bytes = [0u8; 16];
            // SAFETY of the fallback: a request id is a correlation token, not
            // a secret; a failed entropy read degrades to the clock.
            if getrandom::fill(&mut bytes).is_err() {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_nanos())
                    .unwrap_or(0);
                bytes[..16].copy_from_slice(&now.to_le_bytes());
            }
            uuid::Builder::from_random_bytes(bytes)
                .into_uuid()
                .to_string()
        })
}

fn header_map_to_json(headers: &HeaderMap) -> Value {
    let mut out = serde_json::Map::new();
    for (name, value) in headers {
        let value = value.to_str().unwrap_or("").to_owned();
        match out.get_mut(name.as_str()) {
            Some(Value::String(existing)) => {
                existing.push_str(", ");
                existing.push_str(&value);
            }
            _ => {
                out.insert(name.as_str().to_owned(), Value::String(value));
            }
        }
    }
    Value::Object(out)
}

pub(crate) fn query_to_json(query: Option<&str>) -> Value {
    let mut out = serde_json::Map::new();
    for (key, value) in form_urlencoded::parse(query.unwrap_or("").as_bytes()) {
        let key = key.into_owned();
        let value = Value::String(value.into_owned());
        match out.get_mut(&key) {
            Some(Value::Array(items)) => items.push(value),
            Some(existing) => {
                let first = std::mem::take(existing);
                *existing = Value::Array(vec![first, value]);
            }
            None => {
                out.insert(key, value);
            }
        }
    }
    Value::Object(out)
}

/// String-typed transports (path, query, headers) carry scalars as text.
/// When the schema says a top-level property is a number/integer/boolean,
/// convert before validating, so `?page=2` satisfies `{type: integer}`.
pub(crate) fn coerce_scalars(schema: &Value, value: &mut Value) {
    let (Some(properties), Value::Object(object)) =
        (schema.get("properties").and_then(Value::as_object), value)
    else {
        return;
    };
    for (name, property) in properties {
        let Some(current) = object.get_mut(name) else {
            continue;
        };
        let types: Vec<&str> = match property.get("type") {
            Some(Value::String(t)) => vec![t.as_str()],
            Some(Value::Array(ts)) => ts.iter().filter_map(Value::as_str).collect(),
            _ => vec![],
        };
        if let Value::String(text) = current {
            let text = text.clone();
            if types.contains(&"integer")
                && let Ok(n) = text.parse::<i64>()
            {
                *current = json!(n);
            } else if types.contains(&"number")
                && let Ok(n) = text.parse::<f64>()
            {
                *current = json!(n);
            } else if types.contains(&"boolean") {
                match text.as_str() {
                    "true" | "1" => *current = Value::Bool(true),
                    "false" | "0" => *current = Value::Bool(false),
                    _ => {}
                }
            } else if types.contains(&"array") {
                let mut item = Value::String(text);
                if let Some(items) = property.get("items") {
                    coerce_scalars(
                        &json!({ "properties": { "x": items } }),
                        &mut json!({ "x": item.clone() }),
                    );
                    let mut wrapped = json!({ "x": item });
                    coerce_scalars(&json!({ "properties": { "x": items } }), &mut wrapped);
                    item = wrapped["x"].take();
                }
                *current = Value::Array(vec![item]);
            }
        } else if let Value::Array(items) = current
            && let Some(item_schema) = property.get("items")
        {
            for item in items.iter_mut() {
                let mut wrapped = json!({ "x": item.clone() });
                coerce_scalars(&json!({ "properties": { "x": item_schema } }), &mut wrapped);
                *item = wrapped["x"].take();
            }
        }
    }
}

/// The validator's message, minus the parts a client cannot use: a format
/// regex is the schema's business, "does not match the expected format" is
/// the client's.
fn readable_issue(error: &jsonschema::ValidationError<'_>) -> String {
    let text = error.to_string();
    if let Some(idx) = text.find(" does not match \"") {
        let pattern = &text[idx + " does not match \"".len()..];
        if pattern.len() > 24 {
            return format!("{} does not match the expected format", &text[..idx]);
        }
    }
    text
}

/// Equal without an early exit on the first differing byte.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// The JSON pointer of an issue. A missing required property is reported by
/// JSON Schema at the *object* (`""`), by Zod at the property (`/name`): a
/// form wants the field, so the pointer names it — the same path the world
/// would have given (GUIDE §4: the paths are the same wherever the check ran).
fn issue_path(error: &jsonschema::ValidationError<'_>) -> String {
    let base = error.instance_path().to_string();
    match error.kind() {
        jsonschema::error::ValidationErrorKind::Required { property } => {
            let name = property
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| property.to_string());
            format!("{base}/{}", name.replace('~', "~0").replace('/', "~1"))
        }
        _ => base,
    }
}

fn validate(slot: &str, validator: &jsonschema::Validator, value: &Value) -> Result<(), Reply> {
    let issues: Vec<Value> = validator
        .iter_errors(value)
        .map(|e| json!({ "path": issue_path(&e), "message": readable_issue(&e) }))
        .collect();
    if issues.is_empty() {
        Ok(())
    } else {
        Err(Reply::error(
            StatusCode::BAD_REQUEST,
            "validation_failed",
            format!("{slot} failed validation"),
        )
        .with_details(json!({ "slot": slot, "issues": issues })))
    }
}

impl HttpHost {
    pub fn new(runtime: Arc<Runtime>, config: HttpConfig) -> Arc<Self> {
        Arc::new(Self {
            runtime,
            config,
            compiled: RwLock::new(None),
            stats: crate::observability::HttpStats::default(),
            draining: std::sync::atomic::AtomicBool::new(false),
        })
    }

    /// The process is leaving: from now on `/_usai/ready` answers 503 and
    /// responses carry `Connection: close`. The listener stays open for the
    /// grace period the caller chooses, so a load balancer's health check
    /// sees the change before connections start being refused.
    pub fn begin_draining(&self) {
        self.draining
            .store(true, std::sync::atomic::Ordering::Release);
    }

    pub fn is_draining(&self) -> bool {
        self.draining.load(std::sync::atomic::Ordering::Acquire)
    }

    pub fn config(&self) -> &HttpConfig {
        &self.config
    }

    /// Builds the active revision's routing table now, instead of on the
    /// first request. Compiling it is definition-lifetime work (route
    /// matcher, one validator per contract slot) and it was measured at
    /// most of a first request's cost on a small application; an operator
    /// who has just been told the instance is ready should not be handing
    /// that bill to the first caller. Quiet: a revision that cannot serve
    /// HTTP reports itself on the request path as before.
    pub fn warm(&self) {
        let _ = self.compiled();
    }

    /// Definition-lifetime work, done once per active revision.
    fn compiled(&self) -> Result<Arc<CompiledRevision>, Reply> {
        let revision = self.runtime.active().map_err(|_| {
            Reply::error(
                StatusCode::SERVICE_UNAVAILABLE,
                "no_active_revision",
                "no application revision is active",
            )
        })?;
        if let Some(existing) = self.compiled.read().expect("compiled poisoned").as_ref()
            && existing.revision.id == revision.id
        {
            return Ok(Arc::clone(existing));
        }
        let built = Arc::new(CompiledRevision::build(revision).map_err(|e| {
            tracing::error!(error = %e, "revision has an invalid HTTP definition");
            Reply::error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "invalid_definition",
                "the active revision cannot serve HTTP",
            )
        })?);
        *self.compiled.write().expect("compiled poisoned") = Some(Arc::clone(&built));
        Ok(built)
    }

    pub async fn handle(self: Arc<Self>, mut request: Request<Incoming>) -> HttpResponse {
        // Runtime-owned surfaces are not application traffic.
        let internal = request.uri().path().starts_with("/_usai/");
        let started = std::time::Instant::now();
        // One id per request, the client's when it sent a sane one, minted
        // otherwise: it rides into the world's headers (`ctx.requestId`, the
        // world's log lines, outbound calls) and back out on the response.
        let request_id = request_id_for(request.headers());
        if let Ok(v) = HeaderValue::from_str(&request_id) {
            request.headers_mut().insert(REQUEST_ID, v.clone());
        }
        let mut response = match self.pipeline(request).await {
            Ok(mut response) => {
                if !internal {
                    self.stats.record_with(
                        response.status().as_u16(),
                        false,
                        Some(started.elapsed()),
                        None,
                    );
                }
                if self.config.expose_diagnostics
                    && let Ok(value) = HeaderValue::from_str(&format!(
                        "{:.3}",
                        started.elapsed().as_secs_f64() * 1000.0
                    ))
                {
                    response.headers_mut().insert("x-usai-server-ms", value);
                }
                response
            }
            Err(reply) => {
                if !internal {
                    let reason = reply.before_world.then(|| {
                        crate::observability::Rejection::from_code(
                            reply.body["error"]["code"].as_str().unwrap_or(""),
                        )
                    });
                    // A refusal decided before a world existed is not a
                    // served request: it is counted under `rejections`, and
                    // stays out of the latency histogram so a flood of bad
                    // requests cannot make the p99 look better.
                    self.stats.record_with(
                        reply.status.as_u16(),
                        reply.before_world,
                        (!reply.before_world).then(|| started.elapsed()),
                        reason,
                    );
                }
                let mut response = json_response(reply.status, &reply.body);
                for (name, value) in &reply.headers {
                    if let Ok(v) = HeaderValue::from_str(value) {
                        response.headers_mut().insert(*name, v);
                    }
                }
                response
            }
        };
        if !internal && let Ok(v) = HeaderValue::from_str(&request_id) {
            response.headers_mut().insert(REQUEST_ID, v);
        }
        // The application's own headers (`defineApp({ headers })`) on every
        // application response, a handler's value of the same name winning.
        if !internal
            && response.status() != StatusCode::SWITCHING_PROTOCOLS
            && let Ok(compiled) = self.compiled()
            && !compiled.headers.is_empty()
        {
            let headers = response.headers_mut();
            for (name, value) in &compiled.headers {
                if !headers.contains_key(name) {
                    headers.insert(name.clone(), value.clone());
                }
            }
        }
        // A 101 hands the connection to the socket pump; everything else
        // closes after this response while the process is on its way out.
        if self.is_draining() && response.status() != StatusCode::SWITCHING_PROTOCOLS {
            response
                .headers_mut()
                .insert(header::CONNECTION, HeaderValue::from_static("close"));
        }
        response
    }

    pub fn runtime(&self) -> &Arc<Runtime> {
        &self.runtime
    }

    /// The `/_usai/` paths a dedicated status listener actually serves, in
    /// the order an operator reads them. `USAI_SURFACES_OFF` removes them
    /// from here too: the startup banner is the first thing read when
    /// verifying a hardened deployment, and it used to advertise a surface
    /// that answered 404.
    pub fn internal_surfaces(&self) -> Vec<&'static str> {
        [
            ("status", "/_usai/status"),
            ("metrics", "/_usai/metrics"),
            ("live", "/_usai/live"),
            ("ready", "/_usai/ready"),
            ("docs", "/_usai/docs"),
        ]
        .into_iter()
        .filter(|(surface, _)| !self.config.surfaces_off.iter().any(|s| s == surface))
        .map(|(_, path)| path)
        .collect()
    }

    /// The runtime-owned surfaces under `/_usai/`: status, metrics,
    /// liveness, readiness (when `status` is on) and the API reference (when
    /// `docs` is on). `None` for any other path. Served on the application
    /// listener with `--status`, or on their own listener with
    /// `--status-addr` (`serve_internal`), where they belong in production.
    pub async fn internal(
        &self,
        uri: &http::Uri,
        headers: &http::HeaderMap,
        status: bool,
        docs: bool,
    ) -> Option<HttpResponse> {
        let path = uri.path();
        if !path.starts_with("/_usai/") {
            return None;
        }
        let surface = match path {
            "/_usai/status" => "status",
            "/_usai/metrics" => "metrics",
            "/_usai/live" => "live",
            "/_usai/ready" => "ready",
            "/_usai/docs" | "/_usai/docs/" | "/_usai/openapi.json" => "docs",
            _ => "",
        };
        // Whether *this* listener serves it at all is decided before anything
        // else: a public listener that answered 401 for `/_usai/status` would
        // be telling the world there is a protected operator surface here,
        // while `/_usai/live` on the same listener says 404. One answer, and
        // it is the one a listener that does not serve the surface owes:
        // nothing is here.
        let served = match surface {
            "status" | "metrics" | "live" | "ready" => status,
            "docs" => docs,
            _ => false,
        };
        if !served {
            return None;
        }
        if self.config.surfaces_off.iter().any(|s| s == surface) {
            return Some(json_response(
                StatusCode::NOT_FOUND,
                &json!({ "error": { "code": "route_not_found", "message": format!("no route matches GET {path}") } }),
            ));
        }
        // The probes stay open, and so does the reference's HTML shell (a
        // static page that reveals nothing and cannot carry a header from a
        // browser — it asks for the token and fetches the document with it);
        // everything else on this surface is the operator's and takes the
        // status token when one is configured.
        if path != "/_usai/live"
            && path != "/_usai/ready"
            && path != "/_usai/docs"
            && path != "/_usai/docs/"
            && let Some(token) = &self.config.status_token
        {
            let presented = headers
                .get(header::AUTHORIZATION)
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.strip_prefix("Bearer "))
                .map(str::trim)
                .unwrap_or("");
            if !constant_time_eq(presented.as_bytes(), token.as_bytes()) {
                return Some(json_response(
                    StatusCode::UNAUTHORIZED,
                    &json!({ "error": { "code": "unauthorized", "message": "this surface requires the status token (Authorization: Bearer …; USAI_STATUS_TOKEN)" } }),
                ));
            }
        }
        if status {
            match path {
                "/_usai/status" => {
                    let mut status =
                        serde_json::to_value(self.runtime.status()).unwrap_or(Value::Null);
                    status["http"] =
                        serde_json::to_value(self.stats.snapshot()).unwrap_or(Value::Null);
                    return Some(json_response(StatusCode::OK, &status));
                }
                "/_usai/metrics" => {
                    let text = crate::observability::render_prometheus(
                        &self.runtime.status(),
                        Some(&self.stats.snapshot()),
                    );
                    return Some(
                        Response::builder()
                            .status(StatusCode::OK)
                            .header(
                                header::CONTENT_TYPE,
                                "text/plain; version=0.0.4; charset=utf-8",
                            )
                            .body(Full::new(Bytes::from(text)).boxed())
                            .expect("static response"),
                    );
                }
                // Liveness: the process answers. Readiness: an active
                // revision exists and every resource it bound answers a
                // bounded probe (PostgreSQL: `SELECT 1` on a leased
                // connection, 1 s). An orchestrator routes traffic on ready
                // and restarts on live.
                "/_usai/live" => {
                    return Some(json_response(StatusCode::OK, &json!({ "live": true })));
                }
                "/_usai/ready" => {
                    if self.is_draining() {
                        return Some(json_response(
                            StatusCode::SERVICE_UNAVAILABLE,
                            &json!({ "ready": false, "reason": "draining" }),
                        ));
                    }
                    let Ok(revision) = self.runtime.active() else {
                        return Some(json_response(
                            StatusCode::SERVICE_UNAVAILABLE,
                            &json!({ "ready": false, "reason": "no active revision" }),
                        ));
                    };
                    let resources = revision.resources();
                    let mut failed = serde_json::Map::new();
                    for name in resources.names() {
                        // A resource bounds its own probe (and folds the
                        // result into its health, which is what the
                        // `ready` gauge and the alert read). This is the
                        // backstop for one that does not: it answers the
                        // orchestrator, but it cannot tell the resource
                        // anything, so it is deliberately looser.
                        if let Some(manager) = resources.get(name)
                            && let Err(reason) = tokio::time::timeout(
                                std::time::Duration::from_secs(3),
                                manager.probe(),
                            )
                            .await
                            .unwrap_or_else(|_| Err("probe did not answer within 3 s".into()))
                        {
                            failed.insert(name.to_owned(), Value::String(reason));
                        }
                    }
                    // `resources` names what failed its probe either way;
                    // whether that makes this replica unready is the
                    // deployment's call (`ready_requires_resources`).
                    let ready = failed.is_empty() || !self.config.ready_requires_resources;
                    return Some(json_response(
                        if ready {
                            StatusCode::OK
                        } else {
                            StatusCode::SERVICE_UNAVAILABLE
                        },
                        &json!({ "ready": ready, "revision": revision.id, "resources": failed }),
                    ));
                }
                _ => {}
            }
        }
        if docs {
            let compiled = self.compiled().ok()?;
            // `?profile=public` is the consumer contract (what
            // `usai generate openapi --public` writes); the page and the
            // default are the internal profile.
            let profile = form_urlencoded::parse(uri.query().unwrap_or("").as_bytes())
                .find(|(key, _)| key == "profile")
                .map(|(_, value)| {
                    crate::openapi::Profile::parse(&value).ok_or_else(|| value.into_owned())
                })
                .unwrap_or(Ok(crate::openapi::Profile::Internal));
            let profile = match profile {
                Ok(profile) => profile,
                Err(other) => {
                    return Some(json_response(
                        StatusCode::BAD_REQUEST,
                        &json!({ "error": { "code": "unknown_profile", "message": format!("unknown OpenAPI profile {other:?}; use internal or public") } }),
                    ));
                }
            };
            match path {
                "/_usai/openapi.json" => {
                    return Some(json_response(
                        StatusCode::OK,
                        &crate::openapi::generate_with(
                            &compiled.revision.definition,
                            self.runtime.config(),
                            profile,
                        ),
                    ));
                }
                "/_usai/docs" | "/_usai/docs/" => {
                    // A "docs" URL answers HTML unless the client asks for
                    // JSON explicitly; scripts that want the document use
                    // /_usai/openapi.json (the page says so too).
                    let wants_json = headers
                        .get(header::ACCEPT)
                        .and_then(|v| v.to_str().ok())
                        .is_some_and(|a| {
                            a.contains("application/json") && !a.contains("text/html")
                        });
                    if wants_json {
                        return Some(json_response(
                            StatusCode::OK,
                            &crate::openapi::generate_with(
                                &compiled.revision.definition,
                                self.runtime.config(),
                                profile,
                            ),
                        ));
                    }
                    return Some(
                        Response::builder()
                            .status(StatusCode::OK)
                            .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
                            .body(
                                Full::new(Bytes::from_static(crate::openapi::DOCS_HTML.as_bytes()))
                                    .boxed(),
                            )
                            .expect("static response"),
                    );
                }
                _ => {}
            }
        }
        None
    }

    async fn pipeline(&self, request: Request<Incoming>) -> Result<HttpResponse, Reply> {
        let mut watch = Stopwatch::start();
        let compiled = self.compiled()?;
        let (parts, body) = request.into_parts();
        let path = parts.uri.path().to_owned();

        // 0. runtime-owned surfaces (never application work)
        if parts.method == Method::GET
            && let Some(response) = self
                .internal(
                    &parts.uri,
                    &parts.headers,
                    self.config.serve_status,
                    self.config.serve_docs,
                )
                .await
        {
            return Ok(response);
        }

        // A runtime surface asked of a listener that does not serve it: say
        // where it lives instead of a bare 404.
        if path.starts_with("/_usai/") && !self.config.serve_status && !self.config.serve_docs {
            return Err(Reply::error(
                StatusCode::NOT_FOUND,
                "route_not_found",
                format!(
                    "no route matches {} {path}: the runtime's surfaces (status, metrics, live, ready, docs) are not served on this listener — start with --status to put them here, or --status-addr for their own listener",
                    parts.method
                ),
            ));
        }

        // 1. route
        let matched = compiled.router.at(&path).map_err(|_| {
            Reply::error(
                StatusCode::NOT_FOUND,
                "route_not_found",
                format!("no route matches {} {path}", parts.method),
            )
        })?;
        let accepts = |r: &Route| {
            r.method == parts.method.as_str() || (parts.method == Method::HEAD && r.method == "GET")
        };
        // The literal path matched but has no route for this method: a
        // catch-all that does (`OPTIONS /*any`) serves before the 405.
        let fallback = if matched.value.iter().any(accepts) {
            None
        } else {
            compiled
                .catch_alls
                .at(&path)
                .ok()
                .filter(|m| m.value.iter().any(accepts))
        };
        let matched = fallback.unwrap_or(matched);
        if !matched.value.iter().any(accepts) && matched.value.iter().all(|r| r.catch_all) {
            // Only a catch-all matched, and not for this method: the URL is
            // unknown, not a known resource refusing the method.
            return Err(Reply::error(
                StatusCode::NOT_FOUND,
                "route_not_found",
                format!("no route matches {} {path}", parts.method),
            ));
        }
        let route = matched.value.iter().find(|r| accepts(r)).ok_or_else(|| {
            // RFC 9110 §15.5.6: a 405 names what is allowed.
            let mut allowed: Vec<&str> = matched.value.iter().map(|r| r.method.as_str()).collect();
            if allowed.contains(&"GET") {
                allowed.push("HEAD");
            }
            Reply::error(
                StatusCode::METHOD_NOT_ALLOWED,
                "method_not_allowed",
                format!("{} is not allowed on {path}", parts.method),
            )
            .with_header("allow", allowed.join(", "))
        })?;
        let params: Value = matched
            .params
            .iter()
            .map(|(k, v)| (k.to_owned(), Value::String(v.to_owned())))
            .collect::<serde_json::Map<_, _>>()
            .into();
        let workload = compiled
            .revision
            .definition
            .workload_by_index(route.index)
            .expect("routed index exists");
        // From here the workload is known: every outcome, refusal included,
        // is counted under its id (`usai_http_responses_total{workload}`).
        let workload_id = workload.id.clone();
        watch.lap("route");
        let outcome: Result<HttpResponse, Reply> = async {
            let mut query = query_to_json(parts.uri.query());
            let mut headers = header_map_to_json(&parts.headers);

            // Sockets: an HTTP upgrade, then one world for the connection.
            if route.kind == RouteKind::Socket {
                return self
                    .upgrade_socket(
                        parts,
                        compiled.clone(),
                        route.index,
                        &workload.id,
                        params,
                        query,
                        headers,
                    )
                    .await;
            }

            // 2. decode
            let content_type = parts
                .headers
                .get(header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .to_ascii_lowercase();
            let raw_body = Limited::new(body, self.config.max_body_bytes)
                .collect()
                .await
                .map_err(|_| {
                    Reply::error(
                        StatusCode::PAYLOAD_TOO_LARGE,
                        "payload_too_large",
                        format!(
                            "body exceeds {} bytes (the runtime's request body bound; USAI_MAX_BODY_BYTES raises it)",
                            self.config.max_body_bytes
                        ),
                    )
                })?
                .to_bytes();
            let body_json: Value = if raw_body.is_empty() {
                Value::Null
            } else if route.raw() {
                json!({ "base64": base64::engine::general_purpose::STANDARD.encode(&raw_body) })
            } else if content_type.starts_with("application/json")
                || content_type.ends_with("+json")
            {
                let parsed: Value = serde_json::from_slice(&raw_body).map_err(|e| {
                    Reply::error(
                        StatusCode::BAD_REQUEST,
                        "invalid_json",
                        format!("request body is not valid JSON: {e}"),
                    )
                })?;
                json!({ "json": parsed })
            } else if compiled
                .validators
                .get(&route.index)
                .is_some_and(|v| v.body.is_some())
            {
                // A body arrived for a JSON contract without a JSON media
                // type: say so, instead of validating `null`.
                return Err(Reply::error(
                    StatusCode::UNSUPPORTED_MEDIA_TYPE,
                    "unsupported_media_type",
                    if content_type.is_empty() {
                        "the body has no content-type; this route expects application/json".to_owned()
                    } else {
                        format!("{content_type} is not accepted here; this route expects application/json")
                    },
                ));
            } else if content_type.starts_with("text/")
                || content_type.starts_with("application/x-www-form-urlencoded")
            {
                json!({ "text": String::from_utf8_lossy(&raw_body) })
            } else {
                json!({ "base64": base64::engine::general_purpose::STANDARD.encode(&raw_body) })
            };

            watch.lap("decode");
            // 3. boundary validation, before any world exists (C6)
            let mut params = params;
            // The slots validated here, told to the world so the SDK can
            // skip its own parse where the manifest proved it final
            // (`Contracts::boundary_final`).
            let mut validated: Vec<&'static str> = Vec::new();
            if let Some(SlotValidators {
                params: p,
                query: q,
                headers: h,
                body: b,
                schemas,
            }) = compiled.validators.get(&route.index)
            {
                if let Some(v) = p {
                    if let Some(schema) = &schemas.params {
                        coerce_scalars(schema, &mut params);
                    }
                    validate("params", v, &params)?;
                }
                if let Some(v) = q {
                    if let Some(schema) = &schemas.query {
                        coerce_scalars(schema, &mut query);
                    }
                    validate("query", v, &query)?;
                }
                if let Some(v) = h {
                    if let Some(schema) = &schemas.headers {
                        coerce_scalars(schema, &mut headers);
                    }
                    validate("headers", v, &headers)?;
                }
                if let Some(v) = b {
                    let candidate = body_json.get("json").cloned().unwrap_or(Value::Null);
                    validate("body", v, &candidate)?;
                }
                validated.extend(
                    [("params", p), ("query", q), ("headers", h), ("body", b)]
                        .into_iter()
                        .filter(|(_, v)| v.is_some())
                        .map(|(name, _)| name),
                );
            }

            watch.lap("validate");
            // 4. admit
            let admission = self
                .runtime
                .admit_in_flight(&compiled.revision, &workload.id)
                .map_err(|e| match e {
                    RuntimeError::Admission(_) => Reply::error(
                        StatusCode::SERVICE_UNAVAILABLE,
                        "capacity_exhausted",
                        e.to_string(),
                    ),
                    RuntimeError::NotActive(..) => Reply::error(
                        StatusCode::SERVICE_UNAVAILABLE,
                        "revision_draining",
                        e.to_string(),
                    ),
                    other => Reply::error(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "admission_failed",
                        other.to_string(),
                    ),
                })?;

            // 5. world
            let env: BTreeMap<String, String> = (*compiled.revision.env()).clone();
            let request = json!({
                "method": parts.method.as_str(),
                "path": path,
                "url": parts.uri.to_string(),
                "params": params,
                "query": query,
                "headers": headers,
                "body": body_json,
                "validated": validated,
            });
            if route.kind == RouteKind::Stream {
                return self
                    .run_stream(
                        admission,
                        &workload.id,
                        match &workload.trigger {
                            crate::definition::Trigger::Stream { content_type: Some(ct), .. } => ct.clone(),
                            _ => "text/event-stream".to_owned(),
                        },
                        compiled.clone(),
                        request,
                    )
                    .await;
            }
            let input = json!({ "kind": "http", "env": env, "request": request });
            let cancel = CancellationToken::new();
            // Dropping the request future (client gone) cancels the world.
            let _guard = cancel.clone().drop_guard();
            watch.lap("admit");
            let t_execute = std::time::Instant::now();
            let mut result = self
            .runtime
            .execute(admission, input, cancel)
            .await
            .map_err(|e| {
                tracing::error!(error = %e, workload = %workload.id, "world could not be created");
                Reply::after_world(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "world_creation_failed",
                    "the request could not be executed",
                )
            })?;

            tracing::debug!(
                execute_ms = t_execute.elapsed().as_secs_f64() * 1000.0,
                world_ms = result.duration.as_secs_f64() * 1000.0,
                "pipeline timing"
            );
            watch.lap("execute");
            // 6. encode / commit
            let world_profile = if watch.on {
                std::mem::take(&mut result.profile)
            } else {
                Vec::new()
            };
            let mut response = self.encode(&workload.id, result);
            if watch.on {
                watch.lap("encode");
                if let Some(v) = profile_header(&watch.phases, &world_profile) {
                    response.headers_mut().insert("x-usai-profile", v);
                }
            }
            Ok(response)
        }
        .await;
        match outcome {
            Ok(response) => {
                self.stats
                    .record_workload(&workload_id, response.status().as_u16());
                Ok(response)
            }
            Err(mut reply) => {
                self.stats
                    .record_workload(&workload_id, reply.status.as_u16());
                reply.workload = Some(workload_id);
                Err(reply)
            }
        }
    }

    /// A stream world: the response commits at the first `stream.send`
    /// (or `stream.start`) and the body ends when the handler returns.
    /// If the handler returns before sending anything, it is an ordinary
    /// response.
    async fn run_stream(
        &self,
        admission: crate::runtime::Admission,
        workload: &str,
        content_type: String,
        compiled: Arc<CompiledRevision>,
        request: Value,
    ) -> Result<HttpResponse, Reply> {
        let (sink, head_rx, body_rx) = StreamSink::new(&content_type);
        let cancel = CancellationToken::new();
        let stop = compiled.revision.connections_stop();
        let runtime = Arc::clone(&self.runtime);
        let input = json!({ "kind": "stream", "request": request });
        let world_cancel = cancel.clone();
        let mut task = tokio::spawn(async move {
            runtime
                .execute_opts(
                    admission,
                    input,
                    ExecuteOptions {
                        cancel: world_cancel,
                        stop: Some(stop),
                        attachment: Some(sink),
                    },
                )
                .await
        });
        tokio::select! {
            head = head_rx => match head {
                Ok(head) => {
                    let mut builder = Response::builder().status(StatusCode::from_u16(head.status).unwrap_or(StatusCode::OK));
                    let mut set_by_handler = std::collections::BTreeSet::new();
                    for (name, value) in head.headers {
                        if let Ok(v) = HeaderValue::from_str(&value) {
                            set_by_handler.insert(name.to_ascii_lowercase());
                            builder = builder.header(name.as_str(), v);
                        }
                    }
                    builder = builder.header("x-usai-lifetime", "stream");
                    // An event stream is never cacheable; proxies that buffer
                    // whole responses need telling as well. The handler's
                    // own value for either header wins (no duplicates).
                    if !set_by_handler.contains("cache-control") {
                        builder = builder.header(header::CACHE_CONTROL, "no-cache");
                    }
                    if !set_by_handler.contains("x-accel-buffering") {
                        builder = builder.header("x-accel-buffering", "no");
                    }
                    self.stats.streams.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let body = WorldBody { receiver: body_rx, _guard: cancel.drop_guard() };
                    // The world keeps running; its result is observed by the task.
                    let workload = workload.to_owned();
                    let streams_failed = std::sync::Arc::clone(&self.stats.streams_failed);
                    tokio::spawn(async move {
                        match task.await {
                            Ok(Ok(result)) => {
                                for v in &result.violations {
                                    tracing::warn!(world = %result.world, workload, code = v.code, "{}", v.message);
                                }
                                // The head is out with a 200: a handler that fails now
                                // cannot change the status, so the failure is loud where
                                // it can be — the log and a counter — and the body ends
                                // early (GUIDE §9: end a stream with a sentinel the
                                // client checks for).
                                let failed = match (&result.termination, &result.outcome) {
                                    (crate::world::Termination::Completed, Some(Err(e))) => {
                                        Some(format!("{}: {}", e.name, e.message))
                                    }
                                    (crate::world::Termination::Completed, _) => None,
                                    // The client went away (a closed tab, an
                                    // EventSource reconnecting, a drain): the
                                    // world was cancelled with the connection.
                                    // That is how every event stream ends —
                                    // not a failure, not a counter.
                                    (crate::world::Termination::Cancelled { reason }, _) => {
                                        tracing::debug!(world = %result.world, workload, reason, "stream ended with its client");
                                        None
                                    }
                                    (t, _) => Some(format!("{t:?}")),
                                };
                                if let Some(error) = failed {
                                    streams_failed.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                                    tracing::error!(world = %result.world, workload, error = %error, "stream handler failed after the head was sent; the client received a 200 and a body that ended early");
                                }
                            }
                            Ok(Err(e)) => tracing::error!(workload, error = %e, "stream world failed"),
                            Err(e) => tracing::error!(workload, error = %e, "stream task panicked"),
                        }
                    });
                    Ok(builder.body(BoxBody::new(StreamBody::new(body))).expect("stream response"))
                }
                Err(_) => {
                    // The sink was dropped without a head: the world ended first.
                    let result = task.await.map_err(|e| Reply::after_world(StatusCode::INTERNAL_SERVER_ERROR, "stream_failed", e.to_string()))?;
                    let result = result.map_err(|e| Reply::after_world(StatusCode::INTERNAL_SERVER_ERROR, "world_creation_failed", e.to_string()))?;
                    Ok(self.encode(workload, result))
                }
            },
            finished = &mut task => {
                let result = finished.map_err(|e| Reply::after_world(StatusCode::INTERNAL_SERVER_ERROR, "stream_failed", e.to_string()))?;
                let result = result.map_err(|e| Reply::after_world(StatusCode::INTERNAL_SERVER_ERROR, "world_creation_failed", e.to_string()))?;
                Ok(self.encode(workload, result))
            }
        }
    }

    /// A socket world: 101 Switching Protocols, then one world for the
    /// connection with frames delivered as host completions.
    #[allow(clippy::too_many_arguments)]
    async fn upgrade_socket(
        &self,
        mut parts: http::request::Parts,
        compiled: Arc<CompiledRevision>,
        index: usize,
        workload: &str,
        params: Value,
        query: Value,
        headers: Value,
    ) -> Result<HttpResponse, Reply> {
        let is_upgrade = parts
            .headers
            .get(header::UPGRADE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.eq_ignore_ascii_case("websocket"));
        let key = parts
            .headers
            .get("sec-websocket-key")
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        let (Some(key), true) = (key, is_upgrade) else {
            return Err(Reply::error(
                StatusCode::UPGRADE_REQUIRED,
                "upgrade_required",
                "this route is a WebSocket endpoint",
            ));
        };
        let workload_spec = compiled
            .revision
            .definition
            .workload_by_index(index)
            .expect("routed index exists");
        let admission = self
            .runtime
            .admit_in_flight(&compiled.revision, &workload_spec.id)
            .map_err(|e| match e {
                RuntimeError::Admission(_) => Reply::error(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "capacity_exhausted",
                    e.to_string(),
                ),
                other => Reply::error(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "admission_failed",
                    other.to_string(),
                ),
            })?;
        let accept = tokio_tungstenite::tungstenite::handshake::derive_accept_key(key.as_bytes());
        // The OnUpgrade future lives in the request extensions.
        let on_upgrade = parts
            .extensions
            .remove::<hyper::upgrade::OnUpgrade>()
            .ok_or_else(|| {
                Reply::error(
                    StatusCode::UPGRADE_REQUIRED,
                    "upgrade_required",
                    "connection cannot be upgraded",
                )
            })?;
        let (inbound_tx, inbound_rx) = tokio::sync::mpsc::channel::<Inbound>(64);
        let (outbound_tx, outbound_rx) = tokio::sync::mpsc::channel(64);
        let (link, accepted) = SocketLink::new(inbound_rx, outbound_tx);
        let stop = compiled.revision.connections_stop();
        let pump_stop = stop.clone();
        let cancel = CancellationToken::new();
        let pump_cancel = cancel.clone();
        let idle_timeout = self.config.socket_idle_timeout;
        let runtime = Arc::clone(&self.runtime);
        let request = json!({
            "method": "GET",
            "path": parts.uri.path(),
            "url": parts.uri.to_string(),
            "params": params,
            "query": query,
            "headers": headers,
        });
        let input = json!({ "kind": "socket", "request": request });
        let workload = workload.to_owned();
        let world_workload = workload.clone();
        // The world starts first. The 101 is answered only when the handler
        // accepts the connection (its auth resolver passed — `socket.accept`,
        // or the first recv/send of an older bundle); a handler that fails
        // before that answers as any HTTP request would (401, 500), so the
        // client sees a status, not a bare close frame, and the counters
        // count what happened.
        let mut world = tokio::spawn(async move {
            runtime
                .execute_opts(
                    admission,
                    input,
                    ExecuteOptions {
                        cancel,
                        stop: Some(stop),
                        attachment: Some(link),
                    },
                )
                .await
        });
        let handshake_bound = tokio::time::sleep(self.runtime.config().default_timeout);
        tokio::pin!(handshake_bound);
        let protocol = tokio::select! {
            accepted = accepted => match accepted {
                Ok(protocol) => protocol,
                // The link was dropped without accepting: the world ended;
                // fall through to its outcome below.
                Err(_) => {
                    return match world.await {
                        Ok(Ok(result)) => Ok(self.encode(&world_workload, result)),
                        Ok(Err(e)) => Err(Reply::error(StatusCode::INTERNAL_SERVER_ERROR, "socket_world_failed", e.to_string())),
                        Err(e) => Err(Reply::error(StatusCode::INTERNAL_SERVER_ERROR, "socket_world_failed", e.to_string())),
                    };
                }
            },
            result = &mut world => {
                return match result {
                    Ok(Ok(result)) => Ok(self.encode(&world_workload, result)),
                    Ok(Err(e)) => Err(Reply::error(StatusCode::INTERNAL_SERVER_ERROR, "socket_world_failed", e.to_string())),
                    Err(e) => Err(Reply::error(StatusCode::INTERNAL_SERVER_ERROR, "socket_world_failed", e.to_string())),
                };
            }
            _ = &mut handshake_bound => {
                pump_cancel.cancel();
                return Err(Reply::error(StatusCode::GATEWAY_TIMEOUT, "deadline_exceeded", "the socket handler did not accept the connection within the deadline"));
            }
        };
        tokio::spawn(async move {
            match world.await {
                Ok(Ok(result)) => {
                    for v in &result.violations {
                        tracing::warn!(world = %result.world, workload, code = v.code, "{}", v.message);
                    }
                    if let Some(Err(e)) = &result.outcome {
                        tracing::error!(world = %result.world, workload, name = %e.name, error = %e.message, "socket handler failed");
                    }
                }
                Ok(Err(e)) => tracing::error!(workload, error = %e, "socket world failed"),
                Err(e) => tracing::error!(workload, error = %e, "socket world panicked"),
            }
        });
        tokio::spawn(async move {
            match on_upgrade.await {
                Ok(upgraded) => {
                    super::socket::pump(upgraded, inbound_tx, outbound_rx, pump_stop, idle_timeout)
                        .await;
                }
                Err(e) => {
                    tracing::debug!(error = %e, "websocket upgrade failed");
                    pump_cancel.cancel();
                }
            }
        });
        let mut response = Response::builder()
            .status(StatusCode::SWITCHING_PROTOCOLS)
            .header(header::CONNECTION, "upgrade")
            .header(header::UPGRADE, "websocket")
            .header("sec-websocket-accept", accept);
        if let Some(protocol) = protocol {
            response = response.header("sec-websocket-protocol", protocol);
        }
        Ok(response
            .body(http_body_util::Empty::new().boxed())
            .expect("upgrade response"))
    }

    fn encode(&self, workload: &str, result: WorkResult) -> HttpResponse {
        for violation in &result.violations {
            tracing::warn!(world = %result.world, workload, code = violation.code, "{}", violation.message);
        }
        match result.termination {
            Termination::Completed => {}
            Termination::DeadlineExceeded => {
                return json_response(
                    StatusCode::GATEWAY_TIMEOUT,
                    &json!({ "error": { "code": "deadline_exceeded", "message": "the request did not complete within its deadline" } }),
                );
            }
            Termination::Cancelled { reason } => {
                tracing::debug!(world = %result.world, reason, "request cancelled");
                return json_response(
                    StatusCode::from_u16(499).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
                    &json!({ "error": { "code": "cancelled" } }),
                );
            }
            Termination::Faulted { detail } => {
                tracing::error!(world = %result.world, workload, detail, "world faulted");
                let mut body = json!({ "error": { "code": "runtime_fault", "message": "the request could not be completed" } });
                if self.config.expose_diagnostics {
                    body["error"]["detail"] = Value::String(detail);
                }
                return json_response(StatusCode::INTERNAL_SERVER_ERROR, &body);
            }
        }
        // A handler that returned while a write (or another external side
        // effect) was still in flight produced an answer the runtime cannot
        // stand behind: the operation was cancelled with the world, so a
        // 200 here would report success over lost work. Timers and other
        // pure pending work still let the response commit (with the
        // diagnostic in the log and, in dev, the header).
        if let Some(violation) = result
            .violations
            .iter()
            .find(|v| v.code == "detached_work" && v.side_effects_lost)
        {
            let mut body = json!({ "error": { "code": "detached_work", "message": "the handler returned before an operation it started had completed; that operation was cancelled and its result is unknown" } });
            if self.config.expose_diagnostics {
                body["error"]["detail"] = Value::String(violation.message.clone());
            }
            return json_response(StatusCode::INTERNAL_SERVER_ERROR, &body);
        }
        match result.outcome {
            Some(Ok(value)) => match serde_json::from_value::<GuestHttpOutput>(value) {
                Ok(output) => self.commit(output, &result.violations),
                Err(e) => {
                    tracing::error!(world = %result.world, workload, error = %e, "guest returned an undecodable HTTP output");
                    json_response(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        &json!({ "error": { "code": "invalid_handler_output", "message": "the handler produced an unencodable response" } }),
                    )
                }
            },
            Some(Err(error)) => self.application_error(workload, result.world, error),
            None => json_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                &json!({ "error": { "code": "no_outcome", "message": "the request produced no outcome" } }),
            ),
        }
    }

    fn commit(
        &self,
        output: GuestHttpOutput,
        violations: &[crate::world::LifecycleViolation],
    ) -> HttpResponse {
        let lifecycle = (self.config.expose_diagnostics && !violations.is_empty()).then(|| {
            violations
                .iter()
                .map(|v| v.code)
                .collect::<Vec<_>>()
                .join(",")
        });
        render_output(output, lifecycle)
    }
}

/// The guest's HTTP output as a response: status, headers it set (invalid
/// header values are dropped, never a panic), and a body from `json`,
/// `text` or `base64`, with a content type when the handler set none.
pub(crate) fn render_output(output: GuestHttpOutput, lifecycle: Option<String>) -> HttpResponse {
    {
        let status =
            StatusCode::from_u16(output.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        let mut builder = Response::builder().status(status);
        let mut has_content_type = false;
        for (name, values) in &output.headers {
            if name.eq_ignore_ascii_case("content-type") {
                has_content_type = true;
            }
            for value in values.iter() {
                if let Ok(v) = HeaderValue::from_str(value) {
                    builder = builder.header(name.as_str(), v);
                }
            }
        }
        if let Some(codes) = lifecycle {
            builder = builder.header("x-usai-lifecycle", codes);
        }
        let (bytes, content_type) = if let Some(json) = output.json {
            (
                serde_json::to_vec(&json).unwrap_or_default(),
                "application/json",
            )
        } else if let Some(text) = output.text {
            (text.into_bytes(), "text/plain; charset=utf-8")
        } else if let Some(b64) = output.base64 {
            (
                base64::engine::general_purpose::STANDARD
                    .decode(b64)
                    .unwrap_or_default(),
                "application/octet-stream",
            )
        } else {
            (Vec::new(), "")
        };
        if !has_content_type && !content_type.is_empty() {
            builder = builder.header(header::CONTENT_TYPE, content_type);
        }
        builder
            .body(Full::new(Bytes::from(bytes)).boxed())
            .unwrap_or_else(|_| {
                json_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    &json!({ "error": { "code": "invalid_response_headers" } }),
                )
            })
    }
}

/// Connection-level dependency failures, one line per code per second.
static DEPENDENCY_LOG: crate::observability::RateLimitedLog =
    crate::observability::RateLimitedLog::new(std::time::Duration::from_secs(1));

impl HttpHost {
    /// Known application errors map to their declared status; anything else
    /// is sanitized (C11).
    /// The 500 codes the runtime itself produces and documents; any other
    /// code on a 500 is an operation's internal failure and is reported to
    /// the client as `internal`.
    const RUNTIME_500_CODES: [&'static str; 8] = [
        "internal",
        "detached_work",
        "response_contract_violation",
        "resource_not_declared",
        "unknown_task",
        "unknown_operation",
        "invalid_handler_output",
        "runtime_fault",
    ];

    fn application_error(
        &self,
        workload: &str,
        world: crate::ownership::WorldId,
        error: GuestError,
    ) -> HttpResponse {
        if let Some(usai) = &error.usai {
            let status = usai
                .get("status")
                .and_then(Value::as_u64)
                .and_then(|s| StatusCode::from_u16(s as u16).ok())
                .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
            let raw_code = usai.get("code").and_then(Value::as_str).unwrap_or("error");
            // A 500 whose code is not one of the runtime's own is an
            // operation's failure surfacing through the handler (`sql_22003`,
            // `invalid_param`, …): the SQLSTATE and the parameter position
            // belong in the log, not in the client's hands. Declared
            // application errors are 4xx by contract and pass untouched.
            let code = if status == StatusCode::INTERNAL_SERVER_ERROR
                && !Self::RUNTIME_500_CODES.contains(&raw_code)
                && !self.config.expose_diagnostics
            {
                "internal"
            } else {
                raw_code
            };
            let mut body = json!({ "error": { "code": code, "message": error.message } });
            if status.is_client_error()
                && let Some(details) = usai.get("details")
            {
                body["error"]["details"] = details.clone();
            }
            if status.is_server_error() {
                // The details of a server-side contract failure (which field
                // of the response did not match) are what the developer
                // needs: always in the log, in the response only in dev.
                let details = usai.get("details").cloned().unwrap_or(Value::Null);
                if crate::resource::postgres::connection_level(raw_code) {
                    // The database is down: every request fails the same way,
                    // and a stack per request is noise that buries the one
                    // line that matters. One warning per code per second,
                    // counting what it stands for; `/_usai/ready` and
                    // `resources[].ready` carry the state.
                    if let Some(suppressed) = DEPENDENCY_LOG.allow(raw_code) {
                        tracing::warn!(world = %world, workload, code = raw_code, error = %error.message, suppressed, "dependency unavailable");
                    }
                } else {
                    tracing::error!(world = %world, workload, code = raw_code, error = %error.message, %details, stack = error.stack.as_deref().unwrap_or(""), "application error");
                }
                if self.config.expose_diagnostics {
                    if !details.is_null() {
                        body["error"]["details"] = details;
                    }
                } else {
                    body["error"]["message"] = Value::String("internal error".into());
                }
            }
            return json_response(status, &body);
        }
        tracing::error!(world = %world, workload, name = %error.name, error = %error.message, stack = error.stack.as_deref().unwrap_or(""), "unexpected handler failure");
        let mut body = json!({ "error": { "code": "internal", "message": "internal error" } });
        if self.config.expose_diagnostics {
            body["error"]["message"] = Value::String(format!("{}: {}", error.name, error.message));
            if let Some(stack) = error.stack {
                body["error"]["stack"] = Value::String(stack);
            }
        }
        json_response(StatusCode::INTERNAL_SERVER_ERROR, &body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scalars_are_coerced_per_schema() {
        let schema = json!({ "properties": { "page": { "type": "integer" }, "on": { "type": "boolean" }, "tags": { "type": "array", "items": { "type": "string" } }, "q": { "type": "string" } } });
        let mut value = json!({ "page": "2", "on": "true", "tags": "a", "q": "7" });
        coerce_scalars(&schema, &mut value);
        assert_eq!(
            value,
            json!({ "page": 2, "on": true, "tags": ["a"], "q": "7" })
        );
    }

    #[test]
    fn repeated_query_keys_become_arrays() {
        assert_eq!(
            query_to_json(Some("a=1&a=2&b=x")),
            json!({ "a": ["1", "2"], "b": "x" })
        );
    }
}
