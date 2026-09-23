//! Resources: state or capability whose lifetime is intentionally longer than
//! one world (`GOAL.md` §23, contracts C5/C17/C18).
//!
//! A `ResourceManager` lives at runtime lifetime and is shared across
//! revisions only on identity match (ADR-0011). A world never holds a
//! manager; it performs bounded operations through it, and each operation is
//! an external operation with its own owner and terminal proof.

pub mod cache_local;
pub mod http_client;
pub mod postgres;

use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};

use async_trait::async_trait;
use serde::Serialize;
use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken;

use crate::definition::ResourceSpec;

/// `kind + logical name + normalized configuration fingerprint +
/// compatibility version` (ADR-0011). Two revisions share a manager only
/// when this is equal.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
pub struct ResourceIdentity {
    pub kind: String,
    pub name: String,
    pub fingerprint: String,
    pub compat: u32,
}

impl ResourceIdentity {
    /// The fingerprint covers the declared config plus the resolved values of
    /// the env names the spec lists. Secret values are hashed, never kept.
    pub fn compute(
        spec: &ResourceSpec,
        env: &(dyn for<'a> Fn(&'a str) -> Option<String> + Sync),
        compat: u32,
    ) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(serde_json::to_vec(&spec.config).expect("config serializes"));
        for name in &spec.env {
            hasher.update(name.as_bytes());
            hasher.update(b"=");
            hasher.update(env(name).unwrap_or_default().as_bytes());
            hasher.update(b"\n");
        }
        Self {
            kind: spec.kind.clone(),
            name: spec.name.clone(),
            fingerprint: hex::encode(hasher.finalize())[..16].to_owned(),
            compat,
        }
    }
}

impl std::fmt::Display for ResourceIdentity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}@{}", self.kind, self.name, self.fingerprint)
    }
}

/// Why a physical resource may or may not be reused after an operation (C5).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TerminalProof {
    /// The operation reached a known terminal state; the resource is clean.
    Terminal,
    /// The operation's terminal state is unknown; the resource must be
    /// quarantined and replaced, never reused.
    Ambiguous,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReleaseDecision {
    Reusable,
    Quarantined,
}

impl From<TerminalProof> for ReleaseDecision {
    fn from(proof: TerminalProof) -> Self {
        match proof {
            TerminalProof::Terminal => ReleaseDecision::Reusable,
            TerminalProof::Ambiguous => ReleaseDecision::Quarantined,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ResourceError {
    #[error("unknown resource {0}")]
    Unknown(String),
    #[error("resource {resource} has no method {method}")]
    UnknownMethod { resource: String, method: String },
    #[error("resource {resource} capacity exhausted")]
    Exhausted { resource: String },
    #[error("operation cancelled")]
    Cancelled,
    #[error("{code}: {message}")]
    Operation {
        code: String,
        message: String,
        proof: TerminalProof,
    },
    #[error("resource {0} failed to start: {1}")]
    Startup(String, String),
}

impl ResourceError {
    pub fn code(&self) -> &str {
        match self {
            ResourceError::Unknown(_) => "resource_unknown",
            ResourceError::UnknownMethod { .. } => "resource_unknown_method",
            ResourceError::Exhausted { .. } => "resource_exhausted",
            ResourceError::Cancelled => "cancelled",
            ResourceError::Operation { code, .. } => code,
            ResourceError::Startup(..) => "resource_startup",
        }
    }
}

/// One bounded operation against a resource, performed on behalf of a world.
/// The manager owns the physical work; the world only awaits the outcome.
#[derive(Clone, Debug)]
pub struct ResourceCall {
    pub method: String,
    pub args: serde_json::Value,
}

#[derive(Clone, Debug, Serialize)]
pub struct ResourceStatus {
    pub identity: ResourceIdentity,
    pub ready: bool,
    pub in_use: u32,
    pub max: u32,
    pub quarantined: u64,
    pub detail: BTreeMap<String, serde_json::Value>,
}

#[async_trait]
pub trait ResourceManager: Send + Sync {
    fn identity(&self) -> &ResourceIdentity;
    /// Executes one operation. Implementations lease a physical resource for
    /// the duration, and on every path decide reuse only from terminal
    /// proof: cooperative cancellation must await the original operation's
    /// terminal state; a hard abandonment is `Ambiguous`.
    async fn call(
        &self,
        call: ResourceCall,
        cancel: CancellationToken,
    ) -> Result<serde_json::Value, ResourceError>;
    fn status(&self) -> ResourceStatus;
    /// A bounded readiness probe (`/_usai/ready`): `Ok` when the resource
    /// can serve an operation now. Default: ready.
    async fn probe(&self) -> Result<(), String> {
        Ok(())
    }
    async fn shutdown(&self);
    fn as_any(&self) -> &dyn std::any::Any;
}

/// Constructs managers for a resource kind from a spec.
#[async_trait]
pub trait ResourceProvider: Send + Sync {
    fn kind(&self) -> &str;
    fn compat(&self) -> u32;
    async fn open(
        &self,
        spec: &ResourceSpec,
        identity: ResourceIdentity,
        env: &(dyn for<'a> Fn(&'a str) -> Option<String> + Sync),
    ) -> Result<Arc<dyn ResourceManager>, ResourceError>;
}

/// Runtime-lifetime registry of live managers, keyed by identity.
#[derive(Default)]
pub struct ResourceRegistry {
    providers: RwLock<BTreeMap<String, Arc<dyn ResourceProvider>>>,
    managers: RwLock<BTreeMap<ResourceIdentity, Arc<dyn ResourceManager>>>,
}

impl ResourceRegistry {
    pub fn new() -> Self {
        let registry = Self::default();
        registry.register_provider(Arc::new(cache_local::CacheLocalProvider));
        registry.register_provider(Arc::new(postgres::PostgresProvider));
        registry.register_provider(Arc::new(http_client::HttpClientProvider));
        registry
    }

    pub fn register_provider(&self, provider: Arc<dyn ResourceProvider>) {
        self.providers
            .write()
            .expect("providers poisoned")
            .insert(provider.kind().to_owned(), provider);
    }

    /// Returns the manager for `spec`, opening one only when no live manager
    /// has the same identity.
    pub async fn open(
        &self,
        spec: &ResourceSpec,
        env: &(dyn for<'a> Fn(&'a str) -> Option<String> + Sync),
    ) -> Result<Arc<dyn ResourceManager>, ResourceError> {
        let provider = self
            .providers
            .read()
            .expect("providers poisoned")
            .get(&spec.kind)
            .cloned()
            .ok_or_else(|| ResourceError::Unknown(format!("{} (kind {})", spec.name, spec.kind)))?;
        let identity = ResourceIdentity::compute(spec, env, provider.compat());
        if let Some(existing) = self
            .managers
            .read()
            .expect("managers poisoned")
            .get(&identity)
        {
            tracing::debug!(
                kind = %identity.kind,
                name = %identity.name,
                fingerprint = %identity.fingerprint,
                "resource reused"
            );
            return Ok(Arc::clone(existing));
        }
        // ADR-0011: identity is the config plus the *resolved* environment, so
        // a typo in a URL opens a second pool rather than warning. The
        // fingerprint is the only way an operator can see that happen —
        // "reused" after a deployment means the same resource, "opened" means
        // a new one, and two `opened` for one name means the config moved.
        let manager = provider.open(spec, identity.clone(), env).await?;
        // *After* it opened. Logging the line first meant an INFO
        // "resource opened" immediately above a fatal "failed to start",
        // which is a line an operator greps for and is misled by.
        tracing::info!(
            kind = %identity.kind,
            name = %identity.name,
            fingerprint = %identity.fingerprint,
            "resource opened"
        );
        self.managers
            .write()
            .expect("managers poisoned")
            .insert(identity, Arc::clone(&manager));
        Ok(manager)
    }

    /// Shuts down and forgets managers no live revision binds any more
    /// (`keep` = the identities still bound). A manager lives as long as
    /// some revision needs it (ADR-0011), not for the process lifetime.
    pub async fn prune(&self, keep: &[ResourceIdentity]) {
        let stale: Vec<Arc<dyn ResourceManager>> = {
            let mut managers = self.managers.write().expect("managers poisoned");
            let ids: Vec<ResourceIdentity> = managers
                .keys()
                .filter(|id| !keep.contains(id))
                .cloned()
                .collect();
            ids.into_iter()
                .filter_map(|id| managers.remove(&id))
                .collect()
        };
        for manager in stale {
            tracing::info!(resource = %manager.identity(), "resource released: no revision uses it");
            manager.shutdown().await;
        }
    }

    pub fn statuses(&self) -> Vec<ResourceStatus> {
        self.managers
            .read()
            .expect("managers poisoned")
            .values()
            .map(|m| m.status())
            .collect()
    }

    pub async fn shutdown(&self) {
        let managers: Vec<_> =
            std::mem::take(&mut *self.managers.write().expect("managers poisoned"))
                .into_values()
                .collect();
        for manager in managers {
            manager.shutdown().await;
        }
    }
}

/// The set of managers one revision bound at activation, by logical name.
#[derive(Clone, Default)]
pub struct BoundResources {
    by_name: BTreeMap<String, Arc<dyn ResourceManager>>,
}

impl BoundResources {
    pub fn bind(&mut self, name: &str, manager: Arc<dyn ResourceManager>) {
        self.by_name.insert(name.to_owned(), manager);
    }

    pub fn get(&self, name: &str) -> Option<&Arc<dyn ResourceManager>> {
        self.by_name.get(name)
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.by_name.keys().map(String::as_str)
    }
}
