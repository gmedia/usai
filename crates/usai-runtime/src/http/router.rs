//! Definition-lifetime routing state: built once per revision, reused by
//! every request. Nothing here is rebuilt per request (C13).

use std::collections::BTreeMap;
use std::sync::Arc;

use jsonschema::Validator;
use serde_json::Value;

use crate::definition::{ApplicationDefinition, Contracts, Trigger, WorkloadSpec};
use crate::runtime::Revision;

pub struct Route {
    pub method: String,
    pub index: usize,
    pub raw: bool,
}

/// Compiled JSON Schema validators for one workload's contract slots.
#[derive(Default)]
pub struct SlotValidators {
    pub params: Option<Validator>,
    pub query: Option<Validator>,
    pub headers: Option<Validator>,
    pub body: Option<Validator>,
    /// The raw schemas, kept for scalar coercion of string-typed transports.
    pub schemas: Contracts,
}

pub struct CompiledRevision {
    pub revision: Arc<Revision>,
    pub router: matchit::Router<Vec<Route>>,
    pub validators: BTreeMap<usize, SlotValidators>,
}

#[derive(Debug, thiserror::Error)]
pub enum RouteBuildError {
    #[error("workload {workload}: invalid path {path}: {detail}")]
    Path {
        workload: String,
        path: String,
        detail: String,
    },
    #[error("workload {workload}: invalid {slot} schema: {detail}")]
    Schema {
        workload: String,
        slot: String,
        detail: String,
    },
}

fn compile(
    workload: &WorkloadSpec,
    slot: &str,
    schema: &Option<Value>,
) -> Result<Option<Validator>, RouteBuildError> {
    match schema {
        None => Ok(None),
        Some(schema) => {
            jsonschema::validator_for(schema)
                .map(Some)
                .map_err(|e| RouteBuildError::Schema {
                    workload: workload.id.clone(),
                    slot: slot.to_owned(),
                    detail: e.to_string(),
                })
        }
    }
}

/// matchit uses `{name}` for parameters; the developer surface uses `:name`.
pub fn to_matchit_path(path: &str) -> String {
    path.split('/')
        .map(|segment| match segment.strip_prefix(':') {
            Some(name) => format!("{{{name}}}"),
            None => match segment.strip_prefix('*') {
                Some(name) if !name.is_empty() => format!("{{*{name}}}"),
                _ => segment.to_owned(),
            },
        })
        .collect::<Vec<_>>()
        .join("/")
}

impl CompiledRevision {
    pub fn build(revision: Arc<Revision>) -> Result<Self, RouteBuildError> {
        let definition: &ApplicationDefinition = &revision.definition;
        let mut by_path: BTreeMap<String, Vec<Route>> = BTreeMap::new();
        let mut validators = BTreeMap::new();
        for (index, workload) in definition.workloads().iter().enumerate() {
            let Trigger::Http { method, path, raw } = &workload.trigger else {
                continue;
            };
            by_path
                .entry(to_matchit_path(path))
                .or_default()
                .push(Route {
                    method: method.to_ascii_uppercase(),
                    index,
                    raw: *raw,
                });
            if !raw {
                let c = &workload.contracts;
                validators.insert(
                    index,
                    SlotValidators {
                        params: compile(workload, "params", &c.params)?,
                        query: compile(workload, "query", &c.query)?,
                        headers: compile(workload, "headers", &c.headers)?,
                        body: compile(workload, "body", &c.body)?,
                        schemas: c.clone(),
                    },
                );
            }
        }
        let mut router = matchit::Router::new();
        for (path, routes) in by_path {
            let workload = routes
                .first()
                .and_then(|r| definition.workload_by_index(r.index))
                .map(|w| w.id.clone())
                .unwrap_or_default();
            router
                .insert(path.clone(), routes)
                .map_err(|e| RouteBuildError::Path {
                    workload,
                    path,
                    detail: e.to_string(),
                })?;
        }
        Ok(Self {
            revision,
            router,
            validators,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colon_params_become_matchit_params() {
        assert_eq!(
            to_matchit_path("/users/:id/posts/:postId"),
            "/users/{id}/posts/{postId}"
        );
        assert_eq!(to_matchit_path("/files/*path"), "/files/{*path}");
        assert_eq!(to_matchit_path("/"), "/");
    }
}
