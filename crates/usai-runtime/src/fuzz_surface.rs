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
            Trigger::Http { path, .. } | Trigger::Stream { path, .. } | Trigger::Socket { path } => {
                let _ = router.insert(crate::http::router::to_matchit_path(path), workload.id.clone());
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
        for (line, column) in [(0, 0), (1, 1), (1, 0), (7, 3), (u32::MAX, u32::MAX), (1 << 20, 1 << 20)] {
            let _ = map.lookup(line, column);
        }
        let _ = map.map_stack("Error: x\n    at f (app.js:1:5)\n    at app.js:99999:1");
        let _ = map.map_stack(text);
    }
}
