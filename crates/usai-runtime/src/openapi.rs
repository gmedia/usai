//! OpenAPI 3.1 generated from the ApplicationDefinition (`GOAL.md` §41,
//! contract C8). There is no second description of the API: what is served
//! is what is documented, and what cannot be described (raw endpoints,
//! providers without JSON Schema) is marked opaque rather than invented.

use serde_json::{Map, Value, json};

use crate::definition::{ApplicationDefinition, Trigger, WorkloadSpec};

/// Strips the `$schema` dialect key a provider may include; the document
/// declares its dialect once at the top.
fn clean(schema: &Value) -> Value {
    match schema {
        Value::Object(map) => Value::Object(
            map.iter()
                .filter(|(k, _)| k.as_str() != "$schema")
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
        "required": ["error"],
        "properties": {
            "error": {
                "type": "object",
                "required": ["code", "message"],
                "properties": {
                    "code": { "type": "string" },
                    "message": { "type": "string" },
                    "details": {}
                }
            }
        }
    })
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
    generate_with(definition, &crate::RuntimeConfig::default())
}

/// Same, with the runtime's effective defaults (the deadline a workload
/// inherits when it declares none) so the document says what will happen.
pub fn generate_with(definition: &ApplicationDefinition, config: &crate::RuntimeConfig) -> Value {
    let manifest = definition.manifest();
    let mut paths: Map<String, Value> = Map::new();
    let mut security_schemes: Map<String, Value> = Map::new();

    for workload in definition.workloads() {
        let (method, path, raw, lifetime) = match &workload.trigger {
            Trigger::Http { method, path, raw } => (method.clone(), path.clone(), *raw, "request"),
            Trigger::Stream { method, path } => (method.clone(), path.clone(), false, "stream"),
            Trigger::Socket { path } => ("GET".to_owned(), path.clone(), true, "connection"),
            _ => continue,
        };
        let (method, path, raw) = (&method, &path, &raw);
        let mut operation = json!({
            "operationId": operation_id(workload),
            "summary": workload.name,
            "x-usai-lifetime": lifetime,
        });
        if lifetime == "stream" {
            operation["description"] = json!(
                "Streaming response: the connection stays open until the handler returns. Chunks are text/event-stream by default."
            );
            operation["x-usai-stream"] = json!(true);
        }
        if lifetime == "connection" {
            operation["description"] = json!(
                "WebSocket endpoint: send an HTTP upgrade. Messages follow the declared incoming/outgoing contracts; not described as HTTP bodies."
            );
            operation["x-usai-socket"] = json!(true);
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
                    validated.insert(slot.into(), json!("before-world"));
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
            if let Some(auth) = &workload.auth {
                operation["x-usai-auth"] = json!(auth);
            }
        }
        let mut responses: Map<String, Value> = Map::new();

        if *raw {
            operation["description"] = json!(
                "Raw endpoint: the handler reads exact bytes and writes an arbitrary response. Request and response contracts are not described."
            );
            operation["x-usai-raw"] = json!(true);
            operation["requestBody"] = json!({ "content": { "*/*": {} } });
            responses.insert(
                "default".into(),
                json!({ "description": "Opaque response" }),
            );
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
                operation["description"] = json!(format!(
                    "Contracts validated inside the world by their schema provider (no JSON Schema available): {}.",
                    c.in_world_only.join(", ")
                ));
            }
            for (status, schema) in &c.response {
                responses.insert(
                    status.to_string(),
                    json!({
                        "description": "Success",
                        "content": { "application/json": { "schema": clean(schema) } }
                    }),
                );
            }
            if c.response.is_empty() {
                responses.insert(
                    "200".into(),
                    json!({ "description": "Success", "content": { "application/json": {} } }),
                );
            }
            let validates = c.params.is_some()
                || c.query.is_some()
                || c.headers.is_some()
                || c.body.is_some()
                || !c.in_world_only.is_empty();
            if validates {
                responses.insert("400".into(), json!({ "description": "Boundary validation failed", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/UsaiError" } } } }));
            }
        }
        for error in &workload.errors {
            responses
                .entry(error.status.to_string())
                .or_insert_with(|| json!({ "description": format!("Declared error: {}", error.code), "content": { "application/json": { "schema": { "$ref": "#/components/schemas/UsaiError" } } } }));
        }
        if let Some(auth) = &workload.auth {
            responses
                .entry("401".to_owned())
                .or_insert_with(|| json!({ "description": "Authentication failed", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/UsaiError" } } } }));
            if let Some(spec) = definition.auth(auth) {
                let scheme = match spec.scheme.as_str() {
                    "bearer" => json!({ "type": "http", "scheme": "bearer" }),
                    "header" => {
                        json!({ "type": "apiKey", "in": "header", "name": spec.header.clone().unwrap_or_default() })
                    }
                    _ => {
                        json!({ "type": "apiKey", "in": "header", "name": "authorization", "description": "Custom authentication boundary" })
                    }
                };
                security_schemes.insert(auth.clone(), scheme);
                operation["security"] = json!([{ auth: [] }]);
            }
        }
        responses
            .entry("500".to_owned())
            .or_insert_with(|| json!({ "description": "Unexpected failure (sanitized)", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/UsaiError" } } } }));
        operation["responses"] = Value::Object(responses);

        let entry = paths
            .entry(to_openapi_path(path))
            .or_insert_with(|| json!({}));
        entry[method.to_ascii_lowercase()] = operation;
    }

    let mut components = json!({ "schemas": { "UsaiError": error_schema() } });
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
                Trigger::Service { restart } => ("service", json!({ "restart": restart.mode })),
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
    json!({
        "openapi": "3.1.0",
        "jsonSchemaDialect": "https://json-schema.org/draft/2020-12/schema",
        "info": {
            "title": manifest.name,
            "version": definition.identity(),
            "x-usai-identity": definition.identity(),
            "x-usai-modules": manifest.modules.iter().map(|m| m.name.clone()).collect::<Vec<_>>(),
        },
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
            module: None,
            trigger: Trigger::Http {
                method: "GET".into(),
                path: "/users/:id".into(),
                raw: false,
            },
            contracts: Default::default(),
            errors: vec![],
            auth: None,
            resources: vec![],
            dispatches: vec![],
            publishes: vec![],
            max_concurrency: None,
            timeout_ms: None,
        };
        assert_eq!(operation_id(&w), "getUsersId");
    }
}
