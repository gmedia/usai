//! Usai runtime: a persistent host that gives every unit of application work
//! its natural lifetime.
//!
//! ```text
//! Runtime                    persistent: engine, ledger, resource managers, revisions
//!   └─ Revision              immutable ApplicationDefinition + compiled form + budgets
//!        └─ WorldDriver      one unit of work: fresh guest, single owner, one routing gate
//!             ├─ host ops    external operations with their own owners (ledger records)
//!             └─ resources   leased through a ResourceManager, reused only on terminal proof
//! ```
//!
//! The invariants this crate upholds are listed in `docs/LIFECYCLE-CONTRACTS.md`.

pub mod admission;
pub mod build;
pub mod control;
pub mod db;
pub mod definition;
pub mod engine;
pub mod host_ops;
pub mod http;
pub mod observability;
pub mod openapi;
pub mod ownership;
pub mod resource;
pub mod runtime;
pub mod signing;
pub mod sourcemap;
pub mod workloads;
pub mod world;

pub use definition::{ApplicationDefinition, Code, Manifest};
pub use engine::quickjs::{QuickJsConfig, QuickJsEngine};
pub use engine::wasm::{WasmConfig, WasmEngine};
pub use ownership::{OpId, WorldId};
pub use runtime::{Revision, RevisionId, RevisionState, Runtime, RuntimeConfig, RuntimeError};
pub use world::{LifecycleViolation, Termination, WorkResult};
