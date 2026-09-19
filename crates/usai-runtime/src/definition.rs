//! The immutable application definition.
//!
//! Built once (by `usai build` or a test harness), reused across every
//! execution world of a revision, and read by tooling without executing any
//! business code. Nothing here is mutable after construction.

use std::collections::BTreeMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Manifest format version. Bump when a field changes meaning.
pub const MANIFEST_VERSION: u32 = 1;

/// The three lifetime families of `GOAL.md` §9.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum LifetimeFamily {
    Finite,
    ConnectionBound,
    Persistent,
}

/// What kind of work a workload declares. The kind fixes the default
/// lifetime family; the developer never annotates values as ephemeral.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Trigger {
    Http {
        method: String,
        path: String,
        #[serde(default)]
        raw: bool,
        /// Raw endpoints only: the statuses the handler writes, with a
        /// description each, so the reference can list them.
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        responses: BTreeMap<u16, String>,
    },
    Task,
    Cron {
        schedule: String,
        #[serde(default)]
        timeout_ms: Option<u64>,
        #[serde(default = "default_overlap")]
        overlap: OverlapPolicy,
    },
    Command,
    Service {
        #[serde(default)]
        restart: RestartPolicy,
    },
    Queue {
        topic: String,
        #[serde(default = "default_concurrency")]
        concurrency: u32,
        /// PostgreSQL resource backing the queue (default: the first declared).
        #[serde(default)]
        database: Option<String>,
        #[serde(default)]
        retry: RetryPolicy,
    },
    Socket {
        path: String,
    },
    Stream {
        method: String,
        path: String,
    },
}

fn default_overlap() -> OverlapPolicy {
    OverlapPolicy::Skip
}

fn default_concurrency() -> u32 {
    1
}

/// How a service that ends (returns or throws) is treated.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RestartPolicy {
    /// `never` (default): a service that ends stays ended until the next
    /// revision. `on-failure`: restart when it threw. `always`: restart
    /// whenever it ends.
    #[serde(default = "never")]
    pub mode: String,
    #[serde(default = "thousand")]
    pub backoff_ms: u64,
    /// Upper bound on restarts per revision (0 = unbounded).
    #[serde(default = "ten")]
    pub max_restarts: u32,
}

fn never() -> String {
    "never".into()
}
fn ten() -> u32 {
    10
}

impl Default for RestartPolicy {
    fn default() -> Self {
        Self {
            mode: "never".into(),
            backoff_ms: 1000,
            max_restarts: 10,
        }
    }
}

/// Explicit retry (ADR-0014). Default: one attempt, failure is terminal.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RetryPolicy {
    #[serde(default = "one")]
    pub max_attempts: u32,
    /// `fixed` or `exponential`.
    #[serde(default = "fixed")]
    pub backoff: String,
    #[serde(default = "thousand")]
    pub base_ms: u64,
}

fn one() -> u32 {
    1
}
fn fixed() -> String {
    "fixed".into()
}
fn thousand() -> u64 {
    1000
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 1,
            backoff: "fixed".into(),
            base_ms: 1000,
        }
    }
}

impl RetryPolicy {
    /// Delay before attempt number `attempt` (1-based) is retried.
    pub fn delay_ms(&self, attempt: u32) -> u64 {
        match self.backoff.as_str() {
            "exponential" => self
                .base_ms
                .saturating_mul(1u64 << attempt.saturating_sub(1).min(20)),
            _ => self.base_ms,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum OverlapPolicy {
    Allow,
    #[default]
    Skip,
}

impl Trigger {
    pub fn lifetime(&self) -> LifetimeFamily {
        match self {
            Trigger::Http { .. }
            | Trigger::Task
            | Trigger::Cron { .. }
            | Trigger::Command
            | Trigger::Queue { .. } => LifetimeFamily::Finite,
            Trigger::Socket { .. } | Trigger::Stream { .. } => LifetimeFamily::ConnectionBound,
            Trigger::Service { .. } => LifetimeFamily::Persistent,
        }
    }

    pub fn kind_name(&self) -> &'static str {
        match self {
            Trigger::Http { .. } => "http",
            Trigger::Task => "task",
            Trigger::Cron { .. } => "cron",
            Trigger::Command => "command",
            Trigger::Service { .. } => "service",
            Trigger::Queue { .. } => "queue",
            Trigger::Socket { .. } => "socket",
            Trigger::Stream { .. } => "stream",
        }
    }
}

/// A JSON Schema (draft 2020-12) as extracted at build time, or `None` when
/// the schema provider could not describe itself. `None` means the contract
/// is still validated inside the world by the provider's own validator, but
/// the runtime cannot validate before world creation and generated docs are
/// degraded for it (ADR-0002).
pub type JsonSchema = Option<serde_json::Value>;

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Contracts {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: JsonSchema,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query: JsonSchema,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: JsonSchema,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: JsonSchema,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: JsonSchema,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: JsonSchema,
    /// Response contracts keyed by status code. The lowest declared 2xx is
    /// the default status for a plain return.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub response: BTreeMap<u16, serde_json::Value>,
    /// Which contract slots exist in the source but could not be described as
    /// JSON Schema. Validation for these happens in the world.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub in_world_only: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DeclaredError {
    pub code: String,
    pub status: u16,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WorkloadSpec {
    /// Stable identity inside the revision: `<kind>:<name>`.
    pub id: String,
    pub name: String,
    /// One line for the reference and the OpenAPI `summary`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    /// A paragraph for the reference and the OpenAPI `description`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub module: Option<String>,
    pub trigger: Trigger,
    #[serde(default)]
    pub contracts: Contracts,
    #[serde(default)]
    pub errors: Vec<DeclaredError>,
    /// Name of the auth boundary declaration this workload requires, if any.
    #[serde(default)]
    pub auth: Option<String>,
    /// Resource names this workload declares it uses. Informational for
    /// `inspect`/`graph`; access is not restricted by this list in v0.
    #[serde(default)]
    pub resources: Vec<String>,
    /// Task names this workload dispatches to. Informational for `graph`.
    #[serde(default)]
    pub dispatches: Vec<String>,
    /// Queue topics this workload publishes to. Informational for `graph`.
    #[serde(default)]
    pub publishes: Vec<String>,
    /// Per-workload world budget (ADR-0012). `None` = inherit application budget.
    #[serde(default)]
    pub max_concurrency: Option<u32>,
    /// Per-invocation deadline. `None` = inherit application default.
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

impl WorkloadSpec {
    pub fn lifetime(&self) -> LifetimeFamily {
        self.trigger.lifetime()
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ResourceSpec {
    pub name: String,
    /// Resource kind, e.g. `postgres`, `cache.local`.
    pub kind: String,
    #[serde(default)]
    pub module: Option<String>,
    /// Normalized, secret-free configuration. Secrets are referenced by env
    /// name, never inlined; the fingerprint (ADR-0011) is computed at runtime
    /// from the resolved values.
    #[serde(default)]
    pub config: serde_json::Value,
    /// Env variable names whose resolved values participate in the identity
    /// fingerprint.
    #[serde(default)]
    pub env: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EnvRequirement {
    pub name: String,
    /// `string`, `url`, `secret`, `int`, `bool`, `enum`
    pub kind: String,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub values: Vec<String>,
}

/// Checks a resolved value against its declared kind. Presence is checked
/// by the caller; this is about shape, so a typo fails activation rather
/// than the first request (`GOAL.md` §32).
pub fn validate_env(requirement: &EnvRequirement, value: &str) -> Result<(), String> {
    let problem = match requirement.kind.as_str() {
        "url" if !value.contains("://") => Some("expected a URL".to_owned()),
        "int" if value.parse::<i64>().is_err() => Some("expected an integer".to_owned()),
        "bool" if !matches!(value, "true" | "false" | "1" | "0") => {
            Some("expected true/false".to_owned())
        }
        "enum" if !requirement.values.iter().any(|v| v == value) => {
            Some(format!("expected one of {}", requirement.values.join(", ")))
        }
        _ => None,
    };
    match problem {
        Some(detail) => Err(format!("{}: {detail}", requirement.name)),
        None => Ok(()),
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ModuleSpec {
    pub name: String,
    #[serde(default)]
    pub migrations: Vec<String>,
    #[serde(default)]
    pub seeders: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AuthSpec {
    pub name: String,
    /// `bearer`, `header`, `custom`
    pub scheme: String,
    #[serde(default)]
    pub header: Option<String>,
    /// Where the credential comes from, for the OpenAPI security scheme.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// The serializable manifest. This is what `usai build` writes and what the
/// runtime, `inspect`, and OpenAPI generation read.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub manifest_version: u32,
    pub name: String,
    /// One paragraph about the application, for the reference page and
    /// `info.description` of the OpenAPI document. Not part of the identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub modules: Vec<ModuleSpec>,
    pub workloads: Vec<WorkloadSpec>,
    #[serde(default)]
    pub resources: Vec<ResourceSpec>,
    #[serde(default)]
    pub auth: Vec<AuthSpec>,
    #[serde(default)]
    pub env: Vec<EnvRequirement>,
    /// SHA-256 of the application code the manifest describes.
    pub code_sha256: String,
    /// What produced this artifact, for compatibility diagnostics (never
    /// part of the identity: the same application built by another version
    /// is the same application).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub built_with: Option<BuiltWith>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BuiltWith {
    /// `@sakaladev/usai` version that described the application.
    #[serde(default)]
    pub sdk: Option<String>,
    /// `usai` runtime version that built the artifact.
    #[serde(default)]
    pub runtime: Option<String>,
}

/// This runtime's version, as shipped.
pub const RUNTIME_VERSION: &str = env!("CARGO_PKG_VERSION");

/// The application's executable code in the build pipeline's portable form:
/// one ES module whose default export is the application object.
#[derive(Clone, Debug)]
pub struct Code {
    pub source: Arc<str>,
    pub sha256: String,
}

impl Code {
    pub fn new(source: impl Into<Arc<str>>) -> Self {
        let source = source.into();
        let sha256 = hex::encode(Sha256::digest(source.as_bytes()));
        Self { source, sha256 }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum DefinitionError {
    #[error(
        "this artifact uses manifest format {found} (built with SDK {sdk}, runtime {runtime}); \
         runtime {RUNTIME_VERSION} understands format {MANIFEST_VERSION} only. \
         Rebuild the artifact with this runtime (`usai build`) or run the runtime it was built with."
    )]
    UnsupportedVersion {
        found: u32,
        sdk: String,
        runtime: String,
    },
    #[error("manifest describes code {expected} but the loaded code hashes to {found}")]
    CodeMismatch { expected: String, found: String },
    #[error("duplicate workload id {0}")]
    DuplicateWorkload(String),
    #[error("duplicate resource name {0}")]
    DuplicateResource(String),
    #[error("workload {workload} references undeclared auth boundary {auth}")]
    UnknownAuth { workload: String, auth: String },
    #[error("workload {workload} references undeclared resource {resource}")]
    UnknownResource { workload: String, resource: String },
    #[error("workload {0} has an empty name")]
    EmptyName(String),
}

/// Immutable, validated, shareable. Constructed from a manifest plus the code
/// it describes; the constructor is the only place the two are checked
/// against each other.
#[derive(Debug)]
pub struct ApplicationDefinition {
    manifest: Manifest,
    code: Code,
    /// Workload id -> index into `manifest.workloads`. The index is the
    /// ordinal the guest SDK uses to locate the handler.
    index: BTreeMap<String, usize>,
    /// An engine's serialized compiled form of `code`, produced by
    /// `usai build` (`image.cwasm`). Not part of the identity: the same
    /// application with or without it is the same application.
    precompiled: Option<Precompiled>,
    /// The bundle's source map (`app.js.map`), for readable stack frames.
    /// Not part of the identity either.
    source_map: Option<Arc<crate::sourcemap::SourceMap>>,
}

/// A precompiled form and what it was built with.
#[derive(Clone)]
pub struct Precompiled {
    pub engine: String,
    pub fingerprint: String,
    pub bytes: Arc<[u8]>,
}

impl std::fmt::Debug for Precompiled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Precompiled")
            .field("engine", &self.engine)
            .field("fingerprint", &self.fingerprint)
            .field("bytes", &self.bytes.len())
            .finish()
    }
}

impl ApplicationDefinition {
    pub fn new(manifest: Manifest, code: Code) -> Result<Arc<Self>, DefinitionError> {
        if manifest.manifest_version != MANIFEST_VERSION {
            let built = manifest.built_with.clone().unwrap_or(BuiltWith {
                sdk: None,
                runtime: None,
            });
            return Err(DefinitionError::UnsupportedVersion {
                found: manifest.manifest_version,
                sdk: built.sdk.unwrap_or_else(|| "unknown".into()),
                runtime: built.runtime.unwrap_or_else(|| "unknown".into()),
            });
        }
        if manifest.code_sha256 != code.sha256 {
            return Err(DefinitionError::CodeMismatch {
                expected: manifest.code_sha256.clone(),
                found: code.sha256.clone(),
            });
        }
        let mut index = BTreeMap::new();
        for (i, workload) in manifest.workloads.iter().enumerate() {
            if workload.name.is_empty() {
                return Err(DefinitionError::EmptyName(workload.id.clone()));
            }
            if index.insert(workload.id.clone(), i).is_some() {
                return Err(DefinitionError::DuplicateWorkload(workload.id.clone()));
            }
            if let Some(auth) = &workload.auth
                && !manifest.auth.iter().any(|a| &a.name == auth)
            {
                return Err(DefinitionError::UnknownAuth {
                    workload: workload.id.clone(),
                    auth: auth.clone(),
                });
            }
            for resource in &workload.resources {
                if !manifest.resources.iter().any(|r| &r.name == resource) {
                    return Err(DefinitionError::UnknownResource {
                        workload: workload.id.clone(),
                        resource: resource.clone(),
                    });
                }
            }
        }
        let mut seen = std::collections::BTreeSet::new();
        for resource in &manifest.resources {
            if !seen.insert(&resource.name) {
                return Err(DefinitionError::DuplicateResource(resource.name.clone()));
            }
        }
        Ok(Arc::new(Self {
            manifest,
            code,
            index,
            precompiled: None,
            source_map: None,
        }))
    }

    pub fn name(&self) -> &str {
        &self.manifest.name
    }

    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    pub fn code(&self) -> &Code {
        &self.code
    }

    pub fn precompiled(&self) -> Option<&Precompiled> {
        self.precompiled.as_ref()
    }

    /// Attaches a precompiled form (see `Precompiled`).
    pub fn with_precompiled(self: Arc<Self>, precompiled: Precompiled) -> Arc<Self> {
        Arc::new(Self {
            manifest: self.manifest.clone(),
            code: self.code.clone(),
            index: self.index.clone(),
            precompiled: Some(precompiled),
            source_map: self.source_map.clone(),
        })
    }

    /// Attaches the bundle's source map.
    pub fn with_source_map(self: Arc<Self>, map: Arc<crate::sourcemap::SourceMap>) -> Arc<Self> {
        Arc::new(Self {
            manifest: self.manifest.clone(),
            code: self.code.clone(),
            index: self.index.clone(),
            precompiled: self.precompiled.clone(),
            source_map: Some(map),
        })
    }

    pub fn source_map(&self) -> Option<&Arc<crate::sourcemap::SourceMap>> {
        self.source_map.as_ref()
    }

    /// A guest error with its stack frames mapped to source positions when a
    /// map is attached; unchanged otherwise.
    pub fn map_error(&self, mut error: crate::engine::GuestError) -> crate::engine::GuestError {
        if let (Some(map), Some(stack)) = (&self.source_map, &error.stack) {
            error.stack = Some(map.map_stack(stack));
        }
        error
    }

    pub fn workloads(&self) -> &[WorkloadSpec] {
        &self.manifest.workloads
    }

    pub fn resources(&self) -> &[ResourceSpec] {
        &self.manifest.resources
    }

    pub fn workload(&self, id: &str) -> Option<(usize, &WorkloadSpec)> {
        self.index
            .get(id)
            .map(|&i| (i, &self.manifest.workloads[i]))
    }

    pub fn workload_by_index(&self, index: usize) -> Option<&WorkloadSpec> {
        self.manifest.workloads.get(index)
    }

    pub fn auth(&self, name: &str) -> Option<&AuthSpec> {
        self.manifest.auth.iter().find(|a| a.name == name)
    }

    /// Content identity of the definition: manifest + code. Stable across
    /// hosts and engines (ADR-0005).
    pub fn identity(&self) -> String {
        let mut hasher = Sha256::new();
        // `built_with` is provenance, not identity.
        let mut manifest = self.manifest.clone();
        manifest.built_with = None;
        hasher.update(serde_json::to_vec(&manifest).expect("manifest serializes"));
        hasher.update(self.code.sha256.as_bytes());
        hex::encode(hasher.finalize())[..16].to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(code: &Code) -> Manifest {
        Manifest {
            manifest_version: MANIFEST_VERSION,
            built_with: None,
            description: None,
            name: "t".into(),
            modules: vec![],
            workloads: vec![WorkloadSpec {
                id: "http:GET /x".into(),
                name: "GET /x".into(),
                summary: None,
                description: None,
                module: None,
                trigger: Trigger::Http {
                    method: "GET".into(),
                    path: "/x".into(),
                    raw: false,
                    responses: Default::default(),
                },
                contracts: Contracts::default(),
                errors: vec![],
                auth: None,
                resources: vec![],
                dispatches: vec![],
                publishes: vec![],
                max_concurrency: None,
                timeout_ms: None,
            }],
            resources: vec![],
            auth: vec![],
            env: vec![],
            code_sha256: code.sha256.clone(),
        }
    }

    #[test]
    fn code_mismatch_is_rejected() {
        let code = Code::new("export default {}");
        let mut m = manifest(&code);
        m.code_sha256 = "0".repeat(64);
        assert!(matches!(
            ApplicationDefinition::new(m, code).unwrap_err(),
            DefinitionError::CodeMismatch { .. }
        ));
    }

    #[test]
    fn duplicate_workload_is_rejected() {
        let code = Code::new("export default {}");
        let mut m = manifest(&code);
        m.workloads.push(m.workloads[0].clone());
        assert!(matches!(
            ApplicationDefinition::new(m, code).unwrap_err(),
            DefinitionError::DuplicateWorkload(_)
        ));
    }

    #[test]
    fn identity_is_stable_and_content_addressed() {
        let code = Code::new("export default {}");
        let a = ApplicationDefinition::new(manifest(&code), code.clone()).unwrap();
        let b = ApplicationDefinition::new(manifest(&code), code).unwrap();
        assert_eq!(a.identity(), b.identity());
        let other = Code::new("export default {x:1}");
        let c = ApplicationDefinition::new(manifest(&other), other).unwrap();
        assert_ne!(a.identity(), c.identity());
    }

    #[test]
    fn lifetime_follows_trigger() {
        assert_eq!(Trigger::Task.lifetime(), LifetimeFamily::Finite);
        assert_eq!(
            Trigger::Service {
                restart: RestartPolicy::default()
            }
            .lifetime(),
            LifetimeFamily::Persistent
        );
        assert_eq!(
            Trigger::Socket { path: "/c".into() }.lifetime(),
            LifetimeFamily::ConnectionBound
        );
    }
}
