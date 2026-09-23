//! Entry points for the coverage-guided fuzz targets in `fuzz/` (feature
//! `fuzzing`, never compiled into the runtime otherwise). Each one feeds
//! hostile bytes to a boundary where trust changes hands and asserts only
//! that the runtime does not panic: a build artifact from disk, the guest's
//! HTTP output, the request's query string, a source map.

use serde_json::Value;

use crate::definition::{ApplicationDefinition, Code, Manifest, Trigger};

/// A manifest as written by `usai build`: parsed, turned into a
/// definition, rendered as OpenAPI (both profiles) and its HTTP paths
/// converted for the router and inserted into one.
pub fn manifest(data: &[u8]) {
    let Ok(manifest) = serde_json::from_slice::<Manifest>(data) else {
        return;
    };
    let Ok(definition) = ApplicationDefinition::new(manifest, Code::new("")) else {
        return;
    };
    let config = crate::RuntimeConfig::default();
    let _ = crate::openapi::generate_with(&definition, &config, crate::openapi::Profile::Internal);
    let _ = crate::openapi::generate_with(&definition, &config, crate::openapi::Profile::Public);
    let _ = crate::observability::render_graph(&definition);
    let mut router = matchit::Router::new();
    for workload in definition.workloads() {
        match &workload.trigger {
            Trigger::Http { path, .. }
            | Trigger::Stream { path, .. }
            | Trigger::Socket { path } => {
                let _ = router.insert(
                    crate::http::router::to_matchit_path(path),
                    workload.id.clone(),
                );
            }
            Trigger::Cron { schedule, .. } => {
                let _ = std::str::FromStr::from_str(schedule).map(|_: croner::Cron| ());
            }
            _ => {}
        }
    }
    for requirement in &definition.manifest().env {
        let _ = crate::definition::validate_env(requirement, "1");
        let _ = crate::definition::validate_env(requirement, "");
    }
}

/// The HTTP boundary: a query string into JSON, scalar coercion against a
/// schema, and the guest's output rendered as a response.
pub fn http_boundary(data: &[u8]) {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    let mut parts = text.splitn(3, '\u{1f}');
    let query = parts.next().unwrap_or("");
    let schema = parts.next().unwrap_or("");
    let output = parts.next().unwrap_or("");
    let mut value = crate::http::pipeline::query_to_json(Some(query));
    if let Ok(schema) = serde_json::from_str::<Value>(schema) {
        crate::http::pipeline::coerce_scalars(&schema, &mut value);
    }
    if let Ok(output) = serde_json::from_str::<crate::http::pipeline::GuestHttpOutput>(output) {
        let _ = crate::http::pipeline::render_output(output, Some("detached_work".into()));
    }
}

/// A source map (`app.js.map`): parse, look positions up, map a stack.
pub fn sourcemap(data: &[u8]) {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    if let Some(map) = crate::sourcemap::SourceMap::parse(text) {
        for (line, column) in [
            (0, 0),
            (1, 1),
            (1, 0),
            (7, 3),
            (u32::MAX, u32::MAX),
            (1 << 20, 1 << 20),
        ] {
            let _ = map.lookup(line, column);
        }
        let _ = map.map_stack("Error: x\n    at f (app.js:1:5)\n    at app.js:99999:1");
        let _ = map.map_stack(text);
    }
}

/// The guest's side of the host boundary: whatever a world asks for arrives
/// as `kind\0payload` and is decoded here. A bug in the guest — or a core
/// built from sources we did not write — must not be able to panic the host,
/// so every kind is fed arbitrary bytes and every decode must fail cleanly.
pub fn guest_bridge(data: &[u8]) {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    // The bridge's framing, exactly as `host_call` splits it.
    let (kind, payload) = text.split_once('\0').unwrap_or((text, ""));
    // A log line's own framing: `message\0{fields}`.
    let (message, fields) = payload.split_once('\0').unwrap_or((payload, ""));
    let _ = (message.len(), fields.len());
    if !fields.is_empty() {
        let _ = serde_json::from_str::<Value>(fields);
    }
    // What each builtin kind decodes out of the payload, without running the
    // operation: the timer's duration, a resource call, a crypto request.
    let _: u64 = payload.trim().parse().unwrap_or(0);
    let _ = serde_json::from_str::<Value>(payload).map(|v| {
        // The shapes the host reads off a resource call.
        let _ = v.get("name").and_then(Value::as_str);
        let _ = v.get("method").and_then(Value::as_str);
        let _ = v.get("args").cloned();
        let _ = v.get("sql").and_then(Value::as_str);
        let _ = v.get("params").and_then(Value::as_array).map(|a| a.len());
    });
    // The completion identifiers the guest hands back.
    let _: Result<u32, _> = kind.trim().parse();
    let _: Result<u64, _> = payload.trim().parse();
}

/// The parameters a world sends to PostgreSQL: arbitrary JSON, converted to
/// the wire type the prepared statement declared. The values are the
/// application's, the types are the server's, and the conversion between
/// them is ours — a mismatch must be an error, never a panic.
pub fn postgres_params(data: &[u8]) {
    use tokio_postgres::types::Type;
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    let Ok(value) = serde_json::from_str::<Value>(text) else {
        return;
    };
    // The types an application actually binds against, including the ones
    // whose text form the runtime parses itself.
    for ty in [
        Type::BOOL,
        Type::INT2,
        Type::INT4,
        Type::INT8,
        Type::FLOAT4,
        Type::FLOAT8,
        Type::NUMERIC,
        Type::TEXT,
        Type::VARCHAR,
        Type::BYTEA,
        Type::UUID,
        Type::JSON,
        Type::JSONB,
        Type::DATE,
        Type::TIME,
        Type::TIMESTAMP,
        Type::TIMESTAMPTZ,
        Type::INTERVAL,
        Type::INET,
        Type::TEXT_ARRAY,
        Type::INT4_ARRAY,
        Type::UUID_ARRAY,
    ] {
        let _ = crate::resource::postgres::fuzz_to_sql(0, &ty, &value);
    }
}
