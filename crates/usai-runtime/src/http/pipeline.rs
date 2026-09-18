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

use super::router::{CompiledRevision, RouteKind, SlotValidators};
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
        }
    }
}

pub struct HttpHost {
    runtime: Arc<Runtime>,
    config: HttpConfig,
    compiled: RwLock<Option<Arc<CompiledRevision>>>,
    pub stats: crate::observability::HttpStats,
}

/// A response decided before (or instead of) application work.
struct Reply {
    status: StatusCode,
    body: Value,
    /// Decided before any world existed (routing, validation, admission).
    before_world: bool,
    /// The workload this refusal belongs to, once routing named one.
    workload: Option<String>,
}

impl Reply {
    fn error(status: StatusCode, code: &str, message: impl Into<String>) -> Self {
        Self {
            status,
            body: json!({ "error": { "code": code, "message": message.into() } }),
            before_world: true,
            workload: None,
        }
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

#[derive(Deserialize)]
struct GuestHttpOutput {
    status: u16,
    #[serde(default)]
    headers: BTreeMap<String, String>,
    #[serde(default)]
    json: Option<Value>,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    base64: Option<String>,
}

pub type HttpResponse = Response<BoxBody<Bytes, Infallible>>;

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

fn query_to_json(query: Option<&str>) -> Value {
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
fn coerce_scalars(schema: &Value, value: &mut Value) {
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

fn validate(slot: &str, validator: &jsonschema::Validator, value: &Value) -> Result<(), Reply> {
    let issues: Vec<Value> = validator
        .iter_errors(value)
        .map(|e| json!({ "path": e.instance_path().to_string(), "message": readable_issue(&e) }))
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
        })
    }

    pub fn config(&self) -> &HttpConfig {
        &self.config
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

    pub async fn handle(self: Arc<Self>, request: Request<Incoming>) -> HttpResponse {
        // Runtime-owned surfaces are not application traffic.
        let internal = request.uri().path().starts_with("/_usai/");
        let started = std::time::Instant::now();
        match self.pipeline(request).await {
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
                    self.stats.record_with(
                        reply.status.as_u16(),
                        reply.before_world,
                        Some(started.elapsed()),
                        reason,
                    );
                }
                json_response(reply.status, &reply.body)
            }
        }
    }

    pub fn runtime(&self) -> &Arc<Runtime> {
        &self.runtime
    }

    /// The runtime-owned surfaces under `/_usai/`: status, metrics,
    /// liveness, readiness (when `status` is on) and the API reference (when
    /// `docs` is on). `None` for any other path. Served on the application
    /// listener with `--status`, or on their own listener with
    /// `--status-addr` (`serve_internal`), where they belong in production.
    pub async fn internal(
        &self,
        path: &str,
        headers: &http::HeaderMap,
        status: bool,
        docs: bool,
    ) -> Option<HttpResponse> {
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
                    let Ok(revision) = self.runtime.active() else {
                        return Some(json_response(
                            StatusCode::SERVICE_UNAVAILABLE,
                            &json!({ "ready": false, "reason": "no active revision" }),
                        ));
                    };
                    let resources = revision.resources();
                    let mut failed = serde_json::Map::new();
                    for name in resources.names() {
                        if let Some(manager) = resources.get(name)
                            && let Err(reason) = tokio::time::timeout(
                                std::time::Duration::from_secs(1),
                                manager.probe(),
                            )
                            .await
                            .unwrap_or_else(|_| Err("probe timed out after 1 s".into()))
                        {
                            failed.insert(name.to_owned(), Value::String(reason));
                        }
                    }
                    let ready = failed.is_empty();
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
            match path {
                "/_usai/openapi.json" => {
                    return Some(json_response(
                        StatusCode::OK,
                        &crate::openapi::generate_with(
                            &compiled.revision.definition,
                            self.runtime.config(),
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
        let compiled = self.compiled()?;
        let (parts, body) = request.into_parts();
        let path = parts.uri.path().to_owned();

        // 0. runtime-owned surfaces (never application work)
        if parts.method == Method::GET
            && let Some(response) = self
                .internal(
                    &path,
                    &parts.headers,
                    self.config.serve_status,
                    self.config.serve_docs,
                )
                .await
        {
            return Ok(response);
        }

        // 1. route
        let matched = compiled.router.at(&path).map_err(|_| {
            Reply::error(
                StatusCode::NOT_FOUND,
                "route_not_found",
                format!("no route matches {} {path}", parts.method),
            )
        })?;
        let route = matched
            .value
            .iter()
            .find(|r| {
                r.method == parts.method.as_str()
                    || (parts.method == Method::HEAD && r.method == "GET")
            })
            .ok_or_else(|| {
                Reply::error(
                    StatusCode::METHOD_NOT_ALLOWED,
                    "method_not_allowed",
                    format!("{} is not allowed on {path}", parts.method),
                )
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
                        format!("body exceeds {} bytes", self.config.max_body_bytes),
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
            } else if content_type.starts_with("text/")
                || content_type.starts_with("application/x-www-form-urlencoded")
            {
                json!({ "text": String::from_utf8_lossy(&raw_body) })
            } else {
                json!({ "base64": base64::engine::general_purpose::STANDARD.encode(&raw_body) })
            };

            // 3. boundary validation, before any world exists (C6)
            let mut params = params;
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
            }

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
            });
            if route.kind == RouteKind::Stream {
                return self
                    .run_stream(admission, &workload.id, compiled.clone(), request)
                    .await;
            }
            let input = json!({ "kind": "http", "env": env, "request": request });
            let cancel = CancellationToken::new();
            // Dropping the request future (client gone) cancels the world.
            let _guard = cancel.clone().drop_guard();
            let t_execute = std::time::Instant::now();
            let result = self
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
            // 6. encode / commit
            Ok(self.encode(&workload.id, result))
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
        compiled: Arc<CompiledRevision>,
        request: Value,
    ) -> Result<HttpResponse, Reply> {
        let (sink, head_rx, body_rx) = StreamSink::new("text/event-stream");
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
                    for (name, value) in head.headers {
                        if let Ok(v) = HeaderValue::from_str(&value) {
                            builder = builder.header(name.as_str(), v);
                        }
                    }
                    builder = builder.header("x-usai-lifetime", "stream");
                    let body = WorldBody { receiver: body_rx, _guard: cancel.drop_guard() };
                    // The world keeps running; its result is observed by the task.
                    let workload = workload.to_owned();
                    tokio::spawn(async move {
                        match task.await {
                            Ok(Ok(result)) => {
                                for v in &result.violations {
                                    tracing::warn!(world = %result.world, workload, code = v.code, "{}", v.message);
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
        let link = SocketLink::new(inbound_rx, outbound_tx);
        let stop = compiled.revision.connections_stop();
        let pump_stop = stop.clone();
        let cancel = CancellationToken::new();
        let pump_cancel = cancel.clone();
        let idle_timeout = self.config.socket_idle_timeout;
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
        tokio::spawn(async move {
            match runtime
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
            {
                Ok(result) => {
                    for v in &result.violations {
                        tracing::warn!(world = %result.world, workload, code = v.code, "{}", v.message);
                    }
                    if let Some(Err(e)) = &result.outcome {
                        tracing::error!(world = %result.world, workload, name = %e.name, message = %e.message, "socket handler failed");
                    }
                }
                Err(e) => tracing::error!(workload, error = %e, "socket world failed"),
            }
        });
        Ok(Response::builder()
            .status(StatusCode::SWITCHING_PROTOCOLS)
            .header(header::CONNECTION, "upgrade")
            .header(header::UPGRADE, "websocket")
            .header("sec-websocket-accept", accept)
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
        let status =
            StatusCode::from_u16(output.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        let mut builder = Response::builder().status(status);
        let mut has_content_type = false;
        for (name, value) in &output.headers {
            if name.eq_ignore_ascii_case("content-type") {
                has_content_type = true;
            }
            if let Ok(v) = HeaderValue::from_str(value) {
                builder = builder.header(name.as_str(), v);
            }
        }
        if self.config.expose_diagnostics && !violations.is_empty() {
            builder = builder.header(
                "x-usai-lifecycle",
                violations
                    .iter()
                    .map(|v| v.code)
                    .collect::<Vec<_>>()
                    .join(","),
            );
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

    /// Known application errors map to their declared status; anything else
    /// is sanitized (C11).
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
            let code = usai.get("code").and_then(Value::as_str).unwrap_or("error");
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
                tracing::error!(world = %world, workload, code, message = %error.message, %details, stack = error.stack.as_deref().unwrap_or(""), "application error");
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
        tracing::error!(world = %world, workload, name = %error.name, message = %error.message, stack = error.stack.as_deref().unwrap_or(""), "unexpected handler failure");
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
