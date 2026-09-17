//! The request pipeline. One `HttpHost` per runtime; it caches the compiled
//! routing state for the active revision and rebuilds it only when the
//! active revision changes.

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::sync::{Arc, RwLock};

use base64::Engine as _;
use bytes::Bytes;
use http::{HeaderMap, HeaderValue, Method, Request, Response, StatusCode, header};
use http_body_util::{BodyExt, Full, Limited};
use hyper::body::Incoming;
use serde::Deserialize;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use super::router::{CompiledRevision, SlotValidators};
use crate::engine::GuestError;
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
}

impl Default for HttpConfig {
    fn default() -> Self {
        Self {
            addr: ([127, 0, 0, 1], 3000).into(),
            max_body_bytes: 1024 * 1024,
            expose_diagnostics: false,
            serve_docs: false,
        }
    }
}

pub struct HttpHost {
    runtime: Arc<Runtime>,
    config: HttpConfig,
    compiled: RwLock<Option<Arc<CompiledRevision>>>,
}

/// A response decided before (or instead of) application work.
struct Reply {
    status: StatusCode,
    body: Value,
}

impl Reply {
    fn error(status: StatusCode, code: &str, message: impl Into<String>) -> Self {
        Self {
            status,
            body: json!({ "error": { "code": code, "message": message.into() } }),
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

pub type HttpResponse = Response<Full<Bytes>>;

fn json_response(status: StatusCode, body: &Value) -> HttpResponse {
    let bytes = serde_json::to_vec(body).unwrap_or_else(|_| b"{}".to_vec());
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Full::new(Bytes::from(bytes)))
        .expect("static response")
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

fn validate(slot: &str, validator: &jsonschema::Validator, value: &Value) -> Result<(), Reply> {
    let issues: Vec<Value> = validator
        .iter_errors(value)
        .map(|e| json!({ "path": e.instance_path().to_string(), "message": e.to_string() }))
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
        match self.pipeline(request).await {
            Ok(response) => response,
            Err(reply) => json_response(reply.status, &reply.body),
        }
    }

    async fn pipeline(&self, request: Request<Incoming>) -> Result<HttpResponse, Reply> {
        let compiled = self.compiled()?;
        let (parts, body) = request.into_parts();
        let path = parts.uri.path().to_owned();

        // 0. runtime-owned surfaces (never application work)
        if self.config.serve_docs && parts.method == Method::GET {
            match path.as_str() {
                "/_usai/openapi.json" => {
                    return Ok(json_response(
                        StatusCode::OK,
                        &crate::openapi::generate(&compiled.revision.definition),
                    ));
                }
                "/_usai/docs" | "/_usai/docs/" => {
                    return Ok(Response::builder()
                        .status(StatusCode::OK)
                        .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
                        .body(Full::new(Bytes::from_static(
                            crate::openapi::DOCS_HTML.as_bytes(),
                        )))
                        .expect("static response"));
                }
                _ => {}
            }
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

        // 2. decode
        let mut query = query_to_json(parts.uri.query());
        let mut headers = header_map_to_json(&parts.headers);
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
        } else if route.raw {
            json!({ "base64": base64::engine::general_purpose::STANDARD.encode(&raw_body) })
        } else if content_type.starts_with("application/json") || content_type.ends_with("+json") {
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
            .admit(&compiled.revision, &workload.id)
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
        let input = json!({
            "kind": "http",
            "env": env,
            "request": {
                "method": parts.method.as_str(),
                "path": path,
                "url": parts.uri.to_string(),
                "params": params,
                "query": query,
                "headers": headers,
                "body": body_json,
            }
        });
        let cancel = CancellationToken::new();
        // Dropping the request future (client gone) cancels the world.
        let _guard = cancel.clone().drop_guard();
        let result = self
            .runtime
            .execute(admission, input, cancel)
            .await
            .map_err(|e| {
                tracing::error!(error = %e, workload = %workload.id, "world could not be created");
                Reply::error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "world_creation_failed",
                    "the request could not be executed",
                )
            })?;

        // 6. encode / commit
        Ok(self.encode(&workload.id, result))
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
            .body(Full::new(Bytes::from(bytes)))
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
                tracing::error!(world = %world, workload, code, message = %error.message, stack = error.stack.as_deref().unwrap_or(""), "application error");
                if !self.config.expose_diagnostics {
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
