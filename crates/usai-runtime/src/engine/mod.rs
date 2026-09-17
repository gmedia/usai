//! The engine boundary.
//!
//! Everything above this module speaks in terms of definitions, worlds,
//! operations, and outcomes. Everything below it is an execution substrate.
//! The substrate is an implementation decision (ADR-0015), never part of the
//! public programming model.

use std::any::Any;
use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::definition::{ApplicationDefinition, Code};

pub mod quickjs;

/// The guest bridge script. Its contract is documented at the top of the file
/// and in `docs/GUEST-ABI.md`.
pub const GUEST_BRIDGE: &str = include_str!("guest-bridge.js");

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("compile failed: {0}")]
    Compile(String),
    #[error("instantiate failed: {0}")]
    Instantiate(String),
    #[error("guest fault: {0}")]
    Guest(String),
    #[error("engine capacity exhausted")]
    Capacity,
}

/// What the guest hands back when the invoked handler settles.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(untagged)]
pub enum Outcome {
    Ok { ok: bool, value: serde_json::Value },
    Err { ok: bool, error: GuestError },
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct GuestError {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stack: Option<String>,
    /// Application error contract: `{ code, status, details? }` when the SDK
    /// error helpers produced it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usai: Option<serde_json::Value>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Pending {
    pub count: u32,
    pub kinds: Vec<String>,
}

/// Natives the guest bridge calls. Implemented by the world driver's shared
/// state; the engine only forwards.
pub trait HostBindings: Send + Sync {
    /// Starts a host-owned operation and returns its ledger id, or a value
    /// <= 0 when refused.
    fn start(&self, kind: &str, payload: &str) -> i64;
    fn cancel_op(&self, op: u64);
    fn log(&self, level: &str, message: &str);
}

/// Engine-specific compiled form of a definition (bytecode, image, …). Lives
/// at definition lifetime and is shared by every world of the revision.
pub trait Compiled: Send + Sync + Any {
    fn as_any(&self) -> &dyn Any;
}

#[async_trait]
pub trait Engine: Send + Sync {
    fn name(&self) -> &'static str;
    /// Definition-lifetime work: parse/compile once.
    async fn compile(
        &self,
        definition: &ApplicationDefinition,
    ) -> Result<Arc<dyn Compiled>, EngineError> {
        self.compile_code(definition.code()).await
    }
    /// Same, from code alone (the build phase has no manifest yet).
    async fn compile_code(&self, code: &Code) -> Result<Arc<dyn Compiled>, EngineError>;
    /// World-lifetime work: a fresh instance with the bridge installed and
    /// the application module evaluated to its baseline.
    async fn instantiate(
        &self,
        compiled: &Arc<dyn Compiled>,
        bindings: Arc<dyn HostBindings>,
    ) -> Result<Box<dyn WorldInstance>, EngineError>;
    /// Build-time: evaluates the module in a capability-less instance and
    /// returns `__usai_sdk.describe(__usai_app)` (ADR-0009).
    async fn describe(
        &self,
        compiled: &Arc<dyn Compiled>,
    ) -> Result<serde_json::Value, EngineError>;
    /// Build-time: evaluates the module in a capability-less instance and
    /// returns its default export as JSON (used for `usai.config.ts`).
    async fn export_default(
        &self,
        compiled: &Arc<dyn Compiled>,
    ) -> Result<serde_json::Value, EngineError>;
}

/// Bindings for the build phase: every operation is refused, so a
/// declaration that tries to do I/O fails loudly instead of running.
pub struct RefusingBindings;

impl HostBindings for RefusingBindings {
    fn start(&self, _kind: &str, _payload: &str) -> i64 {
        0
    }
    fn cancel_op(&self, _op: u64) {}
    fn log(&self, level: &str, message: &str) {
        tracing::debug!(level, "{message}");
    }
}

/// One live guest. Single-owner by construction: only the world driver holds
/// it, and every method is `&mut self`.
#[async_trait]
pub trait WorldInstance: Send {
    /// Starts the handler for workload `index` with a JSON-encoded input and
    /// runs the guest until it is idle.
    async fn invoke(&mut self, index: usize, input_json: &str) -> Result<(), EngineError>;
    /// Delivers one operation completion and runs the guest until idle.
    /// Returns whether a pending operation accepted it.
    async fn deliver(&mut self, op: u64, ok: bool, payload: &str) -> Result<bool, EngineError>;
    /// Tells the guest its work was cancelled and runs it until idle.
    async fn cancel(&mut self, reason: &str) -> Result<(), EngineError>;
    async fn outcome(&mut self) -> Result<Option<Outcome>, EngineError>;
    async fn pending(&mut self) -> Result<Pending, EngineError>;
    /// Setting this flag aborts guest execution at its next safe point. Used
    /// by the driver's watchdog for runaway synchronous code.
    fn interrupter(&self) -> Arc<std::sync::atomic::AtomicBool>;
    fn as_any_mut(&mut self) -> &mut dyn Any;
}
