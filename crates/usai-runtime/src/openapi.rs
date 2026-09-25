//! OpenAPI 3.1 generated from the ApplicationDefinition (`GOAL.md` §41,
//! contract C8). There is no second description of the API: what is served
//! is what is documented, and what cannot be described (raw endpoints,
//! providers without JSON Schema) is marked opaque rather than invented.

use serde_json::{Map, Value, json};

use crate::definition::{ApplicationDefinition, Trigger, WorkloadSpec};

/// Strips the `$schema` dialect key a provider may include; the document
/// declares its dialect once at the top.
/// JavaScript's safe-integer bounds, which schema libraries emit for
/// `.int()`: not a contract, noise in the document.
const SAFE_INT: f64 = 9_007_199_254_740_991.0;

fn clean(schema: &Value) -> Value {
    match schema {
        Value::Object(map) => Value::Object(
            map.iter()
                .filter(|(k, v)| {
                    k.as_str() != "$schema"
                        && !(k.as_str() == "minimum" && v.as_f64() == Some(-SAFE_INT))
                        && !(k.as_str() == "maximum" && v.as_f64() == Some(SAFE_INT))
                })
                .map(|(k, v)| (k.clone(), clean(v)))
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(clean).collect()),
        other => other.clone(),
    }
}

/// Splits an object schema into per-property parameters for path/query/header.
fn parameters(location: &str, schema: &Value, always_required: bool) -> Vec<Value> {
    let Some(properties) = schema.get("properties").and_then(Value::as_object) else {
        return vec![];
    };
    let required: Vec<&str> = schema
        .get("required")
        .and_then(Value::as_array)
        .map(|r| r.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    properties
        .iter()
        .map(|(name, property)| {
            let mut p = json!({
                "name": name,
                "in": location,
                "required": always_required || required.contains(&name.as_str()),
                "schema": clean(property),
            });
            if let Some(description) = property.get("description") {
                p["description"] = description.clone();
            }
            p
        })
        .collect()
}

fn error_schema() -> Value {
    json!({
        "type": "object",
        "description": "Every error answer. `code` is stable and machine-readable: the workload's declared codes, plus the runtime's: `validation_failed` (400; `details.slot` names the rejected slot and `details.issues[]` carries `path` as a JSON pointer and `message`), `invalid_json` (400, the body is not JSON), `bad_request` (400, another malformed input such as a cursor), `unauthorized` (401), `route_not_found` (404), `method_not_allowed` (405, with an `Allow` header), `payload_too_large` (413), `unsupported_media_type` (415), `upgrade_required` (426, a WebSocket route without an upgrade), `capacity_exhausted` (503), `unavailable` (503, a dependency), `deadline_exceeded` (504), `internal` (500, message sanitized; an operation's own failure is reported as `internal` too, its detail is in the server log). The application's declared codes are listed per operation.",
        "required": ["error"],
        "properties": {
            "error": {
                "type": "object",
                "required": ["code", "message"],
                "properties": {
                    "code": { "type": "string" },
                    "message": { "type": "string" },
                    "details": { "description": "Code-specific; for validation_failed: { slot, issues: [{ path, message }] }." }
                }
            }
        }
    })
}

/// Appends a runtime note to an operation's description, after whatever
/// the developer wrote.
/// The declared media type of a stream endpoint, `text/event-stream` by default.
fn stream_media_type(workload: &WorkloadSpec) -> &str {
    match &workload.trigger {
        Trigger::Stream {
            content_type: Some(ct),
            ..
        } => ct.as_str(),
        _ => "text/event-stream",
    }
}

fn note(operation: &mut Value, text: &str) {
    let existing = operation
        .get("description")
        .and_then(Value::as_str)
        .map(str::to_owned);
    operation["description"] = json!(match existing {
        Some(d) => format!("{d}\n\n{text}"),
        None => text.to_owned(),
    });
}

/// The reason phrase a consumer expects next to a status — for every status
/// an application declares, not only the successes: a `401` declared in
/// `response:` is "Unauthorized", never "Success".
fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        201 => "Created",
        202 => "Accepted",
        203 => "Non-Authoritative Information",
        204 => "No content",
        205 => "Reset Content",
        206 => "Partial Content",
        300..=399 => match status {
            301 => "Moved Permanently",
            302 => "Found",
            303 => "See Other",
            304 => "Not Modified",
            307 => "Temporary Redirect",
            308 => "Permanent Redirect",
            _ => "Redirection",
        },
        400..=499 => match status {
            400 => "Bad Request",
            401 => "Unauthorized",
            402 => "Payment Required",
            403 => "Forbidden",
            404 => "Not Found",
            405 => "Method Not Allowed",
            406 => "Not Acceptable",
            409 => "Conflict",
            410 => "Gone",
            412 => "Precondition Failed",
            413 => "Payload Too Large",
            415 => "Unsupported Media Type",
            422 => "Unprocessable Content",
            423 => "Locked",
            428 => "Precondition Required",
            429 => "Too Many Requests",
            _ => "Client Error",
        },
        500..=599 => match status {
            500 => "Internal Server Error",
            501 => "Not Implemented",
            502 => "Bad Gateway",
            503 => "Service Unavailable",
            504 => "Gateway Timeout",
            _ => "Server Error",
        },
        _ => "Success",
    }
}

/// Statuses that carry no body by definition.
fn bodiless(status: u16) -> bool {
    matches!(status, 204 | 205 | 304) || (100..200).contains(&status)
}

/// The error envelope narrowed to the codes an operation declares for a
/// status: `error.code` becomes an enum a generated client can switch on.
fn declared_error_schema(codes: &[String]) -> Value {
    json!({
        "allOf": [
            { "$ref": "#/components/schemas/UsaiError" },
            { "type": "object", "properties": { "error": { "type": "object", "properties": { "code": { "type": "string", "enum": codes } } } } }
        ]
    })
}

/// `{id}` path parameters for a route whose params have no schema (a raw
/// route): the path template says they exist, so the document does.
fn path_parameters(path: &str) -> Vec<Value> {
    path.split('/')
        .filter_map(|segment| segment.strip_prefix(':'))
        .map(|name| json!({ "name": name, "in": "path", "required": true, "schema": { "type": "string" } }))
        .collect()
}

fn operation_id(workload: &WorkloadSpec) -> String {
    let mut out = String::new();
    let mut upper = false;
    for ch in workload.name.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(if upper {
                ch.to_ascii_uppercase()
            } else {
                ch.to_ascii_lowercase()
            });
            upper = false;
        } else {
            upper = !out.is_empty();
        }
    }
    out
}

fn to_openapi_path(path: &str) -> String {
    path.split('/')
        .map(|segment| match segment.strip_prefix(':') {
            Some(name) => format!("{{{name}}}"),
            None => match segment.strip_prefix('*') {
                Some(name) if !name.is_empty() => format!("{{{name}}}"),
                _ => segment.to_owned(),
            },
        })
        .collect::<Vec<_>>()
        .join("/")
}

pub fn generate(definition: &ApplicationDefinition) -> Value {
    generate_with(
        definition,
        &crate::RuntimeConfig::default(),
        Profile::Internal,
    )
}

/// Who the document is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Profile {
    /// Everything the runtime knows: the `x-usai-*` extensions (lifetime,
    /// deadline, resources leased, tasks handed off, boundary validation),
    /// the non-HTTP workloads, resources and environment. What `/_usai/docs`
    /// renders and what a team's own tooling reads.
    Internal,
    /// The consumer contract only: paths, parameters, bodies, responses,
    /// security schemes and the declared error codes (folded into the
    /// response descriptions). No `x-usai-*` extension, no workload,
    /// resource or environment inventory. What ships to API consumers.
    Public,
}

impl Profile {
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "internal" => Some(Self::Internal),
            "public" => Some(Self::Public),
            _ => None,
        }
    }
}

/// Same, with the runtime's effective defaults (the deadline a workload
/// inherits when it declares none) so the document says what will happen,
/// and for one audience.
pub fn generate_with(
    definition: &ApplicationDefinition,
    config: &crate::RuntimeConfig,
    profile: Profile,
) -> Value {
    let mut document = generate_internal(definition, config);
    if profile == Profile::Public {
        publish(&mut document);
    }
    document
}

/// Reduce the internal document to the consumer contract, in place.
fn publish(document: &mut Value) {
    fn strip_extensions(value: &mut Value) {
        match value {
            Value::Object(map) => {
                map.retain(|key, _| !key.starts_with("x-usai-"));
                for child in map.values_mut() {
                    strip_extensions(child);
                }
            }
            Value::Array(items) => items.iter_mut().for_each(strip_extensions),
            _ => {}
        }
    }
    // Declared error codes already live in each response's description and
    // its `error.code` enum; nothing to move before the extensions go.
    strip_extensions(document);
}

fn generate_internal(definition: &ApplicationDefinition, config: &crate::RuntimeConfig) -> Value {
    let manifest = definition.manifest();
    let mut paths: Map<String, Value> = Map::new();
    let mut security_schemes: Map<String, Value> = Map::new();
    let mut socket_schemas: Map<String, Value> = Map::new();

    for workload in definition.workloads() {
        let (method, path, raw, lifetime) = match &workload.trigger {
            Trigger::Http {
                method, path, raw, ..
            } => (method.clone(), path.clone(), *raw, "request"),
            Trigger::Stream { method, path, .. } => (method.clone(), path.clone(), false, "stream"),
            Trigger::Socket { path } => ("GET".to_owned(), path.clone(), true, "connection"),
            _ => continue,
        };
        let (method, path, raw) = (&method, &path, &raw);
        let mut operation = json!({
            "operationId": workload.operation_id.clone().unwrap_or_else(|| operation_id(workload)),
            "x-usai-lifetime": lifetime,
        });
        if let Some(summary) = &workload.summary {
            operation["summary"] = json!(summary);
        }
        if let Some(description) = &workload.description {
            operation["description"] = json!(description);
        }
        if lifetime == "stream" {
            let media = stream_media_type(workload);
            note(
                &mut operation,
                &format!(
                    "Streaming response: the connection stays open until the handler returns. Chunks are {media}."
                ),
            );
            operation["x-usai-stream"] = json!(true);
            // Declared events: named components (`<OperationId>Event<Name>`)
            // a client can type its `addEventListener` handlers from.
            if !workload.contracts.events.is_empty() {
                let base = operation["operationId"]
                    .as_str()
                    .unwrap_or("stream")
                    .to_owned();
                let pascal = |s: &str| {
                    let mut c = s.chars();
                    c.next()
                        .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
                        .unwrap_or_default()
                };
                let mut listed = Vec::new();
                let mut events = Map::new();
                for (name, schema) in &workload.contracts.events {
                    let component = format!("{}Event{}", pascal(&base), pascal(name));
                    socket_schemas.insert(component.clone(), clean(schema));
                    events.insert(
                        name.clone(),
                        json!({ "$ref": format!("#/components/schemas/{component}") }),
                    );
                    listed.push(format!("`{name}` → components.schemas.{component}"));
                }
                operation["x-usai-events"] = Value::Object(events);
                note(
                    &mut operation,
                    &format!(
                        "Events (`event:` name → `data:` schema): {}.",
                        listed.join("; ")
                    ),
                );
            }
        }
        if lifetime == "connection" {
            let c = &workload.contracts;
            let incoming = c.message.as_ref().map(clean).unwrap_or(Value::Null);
            let outgoing = c.response.get(&200).map(clean).unwrap_or(Value::Null);
            let credential = match workload
                .auth
                .as_ref()
                .and_then(|a| definition.auth(a))
                .map(|a| (a.scheme.as_str(), a.header.clone(), a.credential.clone()))
            {
                None => String::new(),
                Some(("bearer", _, _)) => " From a browser, `new WebSocket(url, [\"bearer\", token])` carries the credential in Sec-WebSocket-Protocol; a refused credential is answered with the HTTP status (401) before the upgrade.".to_owned(),
                Some(("header", header, _)) => format!(" From a browser, `new WebSocket(url, [\"{0}\", value])` carries the {0} credential in Sec-WebSocket-Protocol; a refused credential is answered with the HTTP status (401) before the upgrade.", header.unwrap_or_default()),
                Some(("cookie", _, Some(c))) => format!(" The browser sends the `{}` cookie with the upgrade request by itself (same-site, or `credentials` allowed by the proxy); a missing or refused cookie is answered with the HTTP status (401) before the upgrade.", c.name),
                Some(_) => " The route is authenticated by a custom resolver that reads the upgrade request; a refused credential is answered with the HTTP status (401) before the upgrade.".to_owned(),
            };
            note(
                &mut operation,
                &format!(
                    "WebSocket endpoint: send an HTTP upgrade.{credential} Messages are JSON text frames: incoming messages must match the `incoming` schema (an invalid one is answered with a validation_failed envelope and dropped, the connection stays open), outgoing messages match `outgoing`. Incoming schema: {}. Outgoing schema: {}. A draining server closes with code 1012.",
                    if incoming.is_null() {
                        "any".to_owned()
                    } else {
                        incoming.to_string()
                    },
                    if outgoing.is_null() {
                        "any".to_owned()
                    } else {
                        outgoing.to_string()
                    },
                ),
            );
            operation["x-usai-socket"] = json!({ "incoming": incoming, "outgoing": outgoing });
            // The message contracts as named components, so a generator
            // that ignores `x-` extensions (and the public profile, which
            // strips them) still gets the types: `<OperationId>Incoming`,
            // `<OperationId>Outgoing`.
            let base = operation["operationId"]
                .as_str()
                .unwrap_or("socket")
                .to_owned();
            let pascal = {
                let mut c = base.chars();
                c.next()
                    .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
                    .unwrap_or_default()
            };
            if !incoming.is_null() {
                socket_schemas.insert(format!("{pascal}Incoming"), incoming.clone());
            }
            if !outgoing.is_null() {
                socket_schemas.insert(format!("{pascal}Outgoing"), outgoing.clone());
            }
            note(
                &mut operation,
                &format!(
                    "Message schemas: components.schemas.{pascal}Incoming (client → server), components.schemas.{pascal}Outgoing (server → client)."
                ),
            );
        }
        if let Some(module) = &workload.module {
            operation["tags"] = json!([module]);
        }
        // What this operation does to the system — the facts a consumer of
        // a Usai application can rely on and that no hand-written document
        // would keep current: the world's lifetime and deadline, which
        // boundary slots are refused before a world exists, the resources
        // leased per operation, the tasks handed off, the declared errors.
        {
            let c = &workload.contracts;
            let mut validated = Map::new();
            for (slot, present) in [
                ("params", c.params.is_some()),
                ("query", c.query.is_some()),
                ("headers", c.headers.is_some()),
                ("body", c.body.is_some()),
            ] {
                if present {
                    // `before-world`: refused at the boundary and final
                    // there. `both`: refused at the boundary, then parsed
                    // again in the world because the schema transforms
                    // (`.toLowerCase()`, `.transform()`) — the handler sees
                    // the transformed value.
                    let final_at_boundary = c.boundary_final.iter().any(|f| f == slot);
                    validated.insert(
                        slot.into(),
                        json!(if final_at_boundary {
                            "before-world"
                        } else {
                            "both"
                        }),
                    );
                }
            }
            for slot in &c.in_world_only {
                validated.insert(slot.clone(), json!("in-world"));
            }
            operation["x-usai-validated"] = Value::Object(validated);
            operation["x-usai-resources"] = Value::Array(
                workload
                    .resources
                    .iter()
                    .map(|name| {
                        let kind = definition
                            .resources()
                            .iter()
                            .find(|r| &r.name == name)
                            .map(|r| r.kind.as_str())
                            .unwrap_or("unknown");
                        json!({ "name": name, "kind": kind, "lease": "per operation" })
                    })
                    .collect(),
            );
            operation["x-usai-dispatches"] = json!(workload.dispatches);
            if !workload.publishes.is_empty() {
                operation["x-usai-publishes"] = json!(workload.publishes);
            }
            operation["x-usai-errors"] = Value::Array(
                workload
                    .errors
                    .iter()
                    .map(|e| json!({ "code": e.code, "status": e.status }))
                    .collect(),
            );
            match workload.timeout_ms {
                Some(ms) => {
                    operation["x-usai-timeout-ms"] = json!(ms);
                    operation["x-usai-timeout-source"] = json!("declared");
                }
                None if lifetime == "request" => {
                    operation["x-usai-timeout-ms"] =
                        json!(config.default_timeout.as_millis() as u64);
                    operation["x-usai-timeout-source"] = json!("default");
                }
                None => {}
            }
            if let Some(n) = workload.max_concurrency {
                operation["x-usai-max-concurrency"] = json!(n);
            }
            if let Some(n) = workload.max_body_bytes {
                operation["x-usai-max-body-bytes"] = json!(n);
            }
            if let Some(auth) = &workload.auth {
                operation["x-usai-auth"] = json!(auth);
            }
        }
        let mut responses: Map<String, Value> = Map::new();

        if lifetime == "connection" {
            // A WebSocket is not a raw HTTP exchange: the only HTTP response
            // is the upgrade (or 426 without one); messages are described by
            // the incoming/outgoing contracts, not as bodies.
            responses.insert(
                "101".into(),
                json!({ "description": "Switching Protocols: the WebSocket is open; messages follow the declared incoming/outgoing contracts" }),
            );
            responses.insert(
                "426".into(),
                json!({ "description": "Upgrade Required: the request was not a WebSocket upgrade" }),
            );
        } else if *raw {
            note(
                &mut operation,
                "Raw endpoint: the handler reads exact bytes and writes the response itself. The request body is not described; the statuses below are the ones the handler declares.",
            );
            operation["x-usai-raw"] = json!(true);
            if !matches!(method.as_str(), "GET" | "HEAD" | "DELETE" | "OPTIONS") {
                operation["requestBody"] = json!({ "content": { "*/*": {} } });
            }
            let params = path_parameters(path);
            if !params.is_empty() {
                operation["parameters"] = Value::Array(params);
            }
            let declared = match &workload.trigger {
                Trigger::Http { responses, .. } => responses.clone(),
                _ => Default::default(),
            };
            if declared.is_empty() {
                responses.insert(
                    "default".into(),
                    json!({ "description": "Opaque response" }),
                );
            }
            for (status, description) in declared {
                responses.insert(status.to_string(), json!({ "description": description }));
            }
        } else {
            let c = &workload.contracts;
            let mut params: Vec<Value> = Vec::new();
            if let Some(schema) = &c.params {
                params.extend(parameters("path", schema, true));
            }
            if let Some(schema) = &c.query {
                params.extend(parameters("query", schema, false));
            }
            if let Some(schema) = &c.headers {
                params.extend(parameters("header", schema, false));
            }
            if !params.is_empty() {
                operation["parameters"] = Value::Array(params);
            }
            if let Some(schema) = &c.body {
                operation["requestBody"] = json!({
                    "required": true,
                    "content": { "application/json": { "schema": clean(schema) } }
                });
            }
            if !c.in_world_only.is_empty() {
                operation["x-usai-validated-in-world"] = json!(c.in_world_only);
                note(
                    &mut operation,
                    &format!(
                        "Contracts validated inside the world by their schema provider (no JSON Schema available): {}.",
                        c.in_world_only.join(", ")
                    ),
                );
            }
            for (status, schema) in &c.response {
                responses.insert(
                    status.to_string(),
                    if bodiless(*status) {
                        json!({ "description": reason(*status) })
                    } else {
                        json!({
                            "description": reason(*status),
                            "content": { "application/json": { "schema": clean(schema) } }
                        })
                    },
                );
            }
            if lifetime == "stream" {
                // The response is the stream itself, not a JSON body.
                let media = stream_media_type(workload);
                let description = if media != "text/event-stream" {
                    "Streamed response; the connection stays open until the handler returns"
                        .to_owned()
                } else if workload.contracts.events.is_empty() {
                    "Event stream; the connection stays open until the handler returns".to_owned()
                } else {
                    format!(
                        "Event stream (events: {}); the connection stays open until the handler returns",
                        workload
                            .contracts
                            .events
                            .keys()
                            .cloned()
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                };
                responses.insert(
                    "200".into(),
                    json!({ "description": description, "content": { media: { "schema": { "type": "string" } } } }),
                );
            } else if c.response.is_empty() {
                responses.insert(
                    "200".into(),
                    json!({ "description": "OK; the handler's return value as JSON (no contract declared), or 204 when it returns nothing", "content": { "application/json": {} } }),
                );
            }
            let validates = c.params.is_some()
                || c.query.is_some()
                || c.headers.is_some()
                || c.body.is_some()
                || !c.in_world_only.is_empty();
            if validates {
                let description = if c.body.is_some() {
                    "Boundary validation failed: code validation_failed, details.slot and details.issues[] (path, message); or invalid_json when the body is not JSON"
                } else {
                    "Boundary validation failed: code validation_failed, details.slot and details.issues[] (path, message)"
                };
                responses.insert("400".into(), json!({ "description": description, "content": { "application/json": { "schema": { "$ref": "#/components/schemas/UsaiError" } } } }));
            }
            if c.body.is_some() {
                responses.insert("413".into(), json!({ "description": "Body larger than the runtime's limit (1 MiB by default): payload_too_large", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/UsaiError" } } } }));
                responses.insert("415".into(), json!({ "description": "The body is not application/json: unsupported_media_type", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/UsaiError" } } } }));
            }
        }
        // Declared errors, typed: one response per status whose `error.code`
        // is the enum of the codes declared for it (`if (res.status === 409)
        // res.error.code` narrows in a generated client). A status the
        // runtime already documents (400 validation) keeps its schema and
        // gains the declared codes in its description.
        let mut by_status: std::collections::BTreeMap<u16, Vec<String>> = Default::default();
        for error in &workload.errors {
            let codes = by_status.entry(error.status).or_default();
            if !codes.contains(&error.code) {
                codes.push(error.code.clone());
            }
        }
        for (status, codes) in by_status {
            let description = format!("{}: code {}", reason(status), codes.join(" | "));
            match responses.get_mut(&status.to_string()) {
                Some(existing) => {
                    let previous = existing["description"].as_str().unwrap_or("").to_owned();
                    existing["description"] =
                        json!(format!("{previous}; also declared: {}", codes.join(" | ")));
                }
                None => {
                    responses.insert(
                        status.to_string(),
                        json!({ "description": description, "content": { "application/json": { "schema": declared_error_schema(&codes) } } }),
                    );
                }
            }
        }
        if let Some(auth) = &workload.auth {
            responses
                .entry("401".to_owned())
                .or_insert_with(|| json!({ "description": "Authentication failed: code unauthorized (the credential is missing, malformed, or the resolver refused it)", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/UsaiError" } } } }));
            if let Some(spec) = definition.auth(auth) {
                let scheme = match spec.scheme.as_str() {
                    "bearer" => Some(json!({ "type": "http", "scheme": "bearer" })),
                    "header" => Some(
                        json!({ "type": "apiKey", "in": "header", "name": spec.header.clone().unwrap_or_default() }),
                    ),
                    // A custom resolver reads the request itself; the document
                    // describes the credential only where the application
                    // declared it (a cookie, a query parameter, a header) and
                    // never invents an `Authorization` header a generated
                    // client would send for nothing.
                    _ => spec
                        .credential
                        .as_ref()
                        .map(|c| json!({ "type": "apiKey", "in": c.location, "name": c.name })),
                };
                match scheme {
                    Some(mut scheme) => {
                        if let Some(description) = &spec.description {
                            scheme["description"] = json!(description);
                        }
                        security_schemes.insert(auth.clone(), scheme);
                        operation["security"] = json!([{ auth: [] }]);
                    }
                    None => {
                        let note = match &spec.description {
                            Some(d) => format!("Authentication: custom scheme `{auth}` — {d}"),
                            None => format!(
                                "Authentication: custom scheme `{auth}`; the application's resolver reads the request itself (declare `credential` on `auth.custom` to document where the credential travels)"
                            ),
                        };
                        let description =
                            operation["description"].as_str().unwrap_or("").to_owned();
                        operation["description"] = json!(if description.is_empty() {
                            note
                        } else {
                            format!("{description}\n\n{note}")
                        });
                    }
                }
            }
        }
        responses
            .entry("500".to_owned())
            .or_insert_with(|| json!({ "description": "Unexpected failure (sanitized)", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/UsaiError" } } } }));
        // Every operation can be refused or time out; a consumer must
        // handle both, so the contract says so.
        responses
            .entry("503".to_owned())
            .or_insert_with(|| json!({ "description": "Refused: capacity_exhausted (retry later) or unavailable (a dependency is down)", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/UsaiError" } } } }));
        if lifetime == "request" {
            responses
                .entry("504".to_owned())
                .or_insert_with(|| json!({ "description": "Deadline exceeded: deadline_exceeded", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/UsaiError" } } } }));
        }
        // Documented response headers: on the status they were declared
        // for, or on every declared status for `"*"` (a generated client
        // learns that `location`, `etag` or `set-cookie` exist).
        if !workload.response_headers.is_empty() {
            let statuses: Vec<String> = responses.keys().cloned().collect();
            for (status, headers) in &workload.response_headers {
                let targets: Vec<&String> = if status == "*" {
                    statuses
                        .iter()
                        .filter(|s| s.starts_with(['2', '3']))
                        .collect()
                } else {
                    statuses.iter().filter(|s| *s == status).collect()
                };
                for target in targets {
                    let response = responses.get_mut(target).expect("listed status");
                    let slot = response
                        .as_object_mut()
                        .expect("response object")
                        .entry("headers")
                        .or_insert_with(|| json!({}));
                    for (name, description) in headers {
                        slot[name] =
                            json!({ "description": description, "schema": { "type": "string" } });
                    }
                }
                if status != "*" && !responses.contains_key(status) {
                    // A header documented for a status the operation never
                    // declared: list the status so the header is not lost.
                    let mut slot = json!({});
                    for (name, description) in headers {
                        slot[name] =
                            json!({ "description": description, "schema": { "type": "string" } });
                    }
                    responses.insert(
                        status.clone(),
                        json!({ "description": reason(status.parse().unwrap_or(200)), "headers": slot }),
                    );
                }
            }
        }
        operation["responses"] = Value::Object(responses);

        let entry = paths
            .entry(to_openapi_path(path))
            .or_insert_with(|| json!({}));
        entry[method.to_ascii_lowercase()] = operation;
    }

    let mut components = json!({ "schemas": { "UsaiError": error_schema() } });
    for (name, schema) in socket_schemas {
        components["schemas"][name] = schema;
    }
    if !security_schemes.is_empty() {
        components["securitySchemes"] = Value::Object(security_schemes);
    }
    // The rest of the application: the work that is not an HTTP operation
    // but that the document's reader will meet (a task an endpoint hands off
    // to, the cron that purges, the queue a request publishes to) and the
    // resources everything leases from.
    let workloads: Vec<Value> = definition
        .workloads()
        .iter()
        .filter(|w| !matches!(w.trigger, Trigger::Http { .. } | Trigger::Stream { .. } | Trigger::Socket { .. }))
        .map(|w| {
            let (kind, detail) = match &w.trigger {
                Trigger::Task => ("task", json!({})),
                Trigger::Cron { schedule, overlap, .. } => ("cron", json!({ "schedule": schedule, "overlap": format!("{overlap:?}").to_lowercase() })),
                Trigger::Command => ("command", json!({})),
                Trigger::Service {
                    restart, exclusive, ..
                } => (
                    "service",
                    json!({ "restart": restart.mode, "exclusive": exclusive }),
                ),
                Trigger::Queue { topic, concurrency, .. } => ("queue", json!({ "topic": topic, "concurrency": concurrency })),
                _ => ("workload", json!({})),
            };
            let mut entry = json!({
                "id": w.id,
                "name": w.name,
                "kind": kind,
                "module": w.module,
                "lifetime": match w.lifetime() {
                    crate::definition::LifetimeFamily::Finite => "finite",
                    crate::definition::LifetimeFamily::ConnectionBound => "connection",
                    crate::definition::LifetimeFamily::Persistent => "persistent",
                },
                "resources": w.resources,
                "dispatches": w.dispatches,
                "publishes": w.publishes,
                "detail": detail,
            });
            if let Some(description) = &w.description {
                entry["description"] = json!(description);
            }
            if let Some(schema) = &w.contracts.input {
                entry["input"] = clean(schema);
            }
            if let Some(schema) = &w.contracts.message {
                entry["message"] = clean(schema);
            }
            entry
        })
        .collect();
    let resources: Vec<Value> = definition
        .resources()
        .iter()
        .map(|r| json!({ "name": r.name, "kind": r.kind, "module": r.module }))
        .collect();
    let env: Vec<Value> = manifest
        .env
        .iter()
        .map(|e| serde_json::to_value(e).unwrap_or(Value::Null))
        .collect();
    let mut info = json!({
        "title": manifest.name,
        "version": definition.identity(),
        "x-usai-identity": definition.identity(),
        "x-usai-modules": manifest.modules.iter().map(|m| m.name.clone()).collect::<Vec<_>>(),
    });
    if let Some(description) = &manifest.description {
        info["description"] = json!(description);
    }
    json!({
        "openapi": "3.1.0",
        "jsonSchemaDialect": "https://json-schema.org/draft/2020-12/schema",
        "info": info,
        "paths": Value::Object(paths),
        "components": components,
        "x-usai-workloads": workloads,
        "x-usai-resources": resources,
        "x-usai-env": env,
    })
}

/// A dependency-free documentation page that renders the generated
/// document client-side. Served by the dev server at `/_usai/docs`.
pub const DOCS_HTML: &str = include_str!("docs.html");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_and_operation_ids() {
        assert_eq!(
            to_openapi_path("/users/:id/posts/:postId"),
            "/users/{id}/posts/{postId}"
        );
        let w = WorkloadSpec {
            id: "http:GET /users/:id".into(),
            name: "GET /users/:id".into(),
            summary: None,
            description: None,
            module: None,
            trigger: Trigger::Http {
                method: "GET".into(),
                path: "/users/:id".into(),
                raw: false,
                responses: Default::default(),
            },
            contracts: Default::default(),
            errors: vec![],
            response_headers: Default::default(),
            operation_id: None,
            auth: None,
            resources: vec![],
            dispatches: vec![],
            publishes: vec![],
            max_concurrency: None,
            max_body_bytes: None,
            timeout_ms: None,
        };
        assert_eq!(operation_id(&w), "getUsersId");
    }
}
