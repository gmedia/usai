//! Host operations: everything the guest can ask the outside world to do.
//!
//! Each operation is started synchronously from a guest native call, owned by
//! a spawned task that holds the ledger record, and completed through the
//! world's completion channel. The owner reaches a terminal state and
//! releases its record whether or not the world is still alive (C4).

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use serde::Deserialize;
use serde_json::json;
use tokio_util::sync::CancellationToken;

use crate::ownership::{Ledger, OpGuard, OpId, WorldId};
use crate::resource::{BoundResources, ResourceCall, ResourceError};

/// What an operation owner reports back. `ok = false` carries a JSON error
/// object the bridge turns into a rejection.
#[derive(Clone, Debug)]
pub struct OpOutcome {
    pub ok: bool,
    pub payload: String,
}

impl OpOutcome {
    pub fn ok(value: &serde_json::Value) -> Self {
        Self {
            ok: true,
            payload: value.to_string(),
        }
    }

    pub fn err(code: &str, status: u16, message: impl Into<String>) -> Self {
        let message = message.into();
        Self {
            ok: false,
            payload: json!({
                "name": "UsaiOperationError",
                "message": message,
                "usai": { "code": code, "status": status },
            })
            .to_string(),
        }
    }

    pub fn from_resource_error(error: &ResourceError) -> Self {
        let status = match error {
            ResourceError::Exhausted { .. } => 503,
            ResourceError::Cancelled => 499,
            _ => 500,
        };
        Self::err(error.code(), status, error.to_string())
    }
}

/// A completion travels with its ledger guard so the record is released only
/// after the driver has had the chance to route it. If the world is gone the
/// message is dropped, the guard drops, and the record is released anyway.
pub struct Completion {
    pub world: WorldId,
    pub op: OpId,
    pub outcome: OpOutcome,
    pub guard: OpGuard,
}

pub type OpFuture = Pin<Box<dyn Future<Output = OpOutcome> + Send>>;

/// Everything an operation may need from the world that started it.
#[derive(Clone)]
pub struct OpContext {
    pub world: WorldId,
    pub op: OpId,
    pub cancel: CancellationToken,
    pub resources: Arc<BoundResources>,
    pub extensions: Arc<OpExtensions>,
}

/// Hooks other subsystems (tasks, cron) register so the op layer does not
/// depend on them. Filled in by the runtime.
#[derive(Default)]
pub struct OpExtensions {
    pub handlers: HashMap<String, Arc<dyn OpHandler>>,
}

pub trait OpHandler: Send + Sync {
    fn start(&self, ctx: OpContext, payload: String) -> Result<OpFuture, OpOutcome>;
}

/// Kinds the runtime itself understands. Extensions may add more.
pub fn start_builtin(kind: &str, ctx: OpContext, payload: String) -> Result<OpFuture, OpOutcome> {
    match kind {
        "timer" => Ok(Box::pin(timer(ctx, payload))),
        "resource" => Ok(Box::pin(resource(ctx, payload))),
        other => match ctx.extensions.handlers.get(other) {
            Some(handler) => Arc::clone(handler).start(ctx, payload),
            None => Err(OpOutcome::err(
                "unknown_operation",
                500,
                format!("the runtime has no operation kind {other}"),
            )),
        },
    }
}

async fn timer(ctx: OpContext, payload: String) -> OpOutcome {
    let ms: u64 = payload.trim().parse().unwrap_or(0);
    tokio::select! {
        _ = tokio::time::sleep(Duration::from_millis(ms)) => OpOutcome::ok(&serde_json::Value::Null),
        _ = ctx.cancel.cancelled() => OpOutcome::err("cancelled", 499, "timer cancelled with its world"),
    }
}

#[derive(Deserialize)]
struct ResourceRequest {
    name: String,
    method: String,
    #[serde(default)]
    args: serde_json::Value,
}

async fn resource(ctx: OpContext, payload: String) -> OpOutcome {
    let request: ResourceRequest = match serde_json::from_str(&payload) {
        Ok(r) => r,
        Err(e) => return OpOutcome::err("invalid_resource_request", 500, e.to_string()),
    };
    let Some(manager) = ctx.resources.get(&request.name) else {
        return OpOutcome::from_resource_error(&ResourceError::Unknown(request.name));
    };
    let call = ResourceCall {
        method: request.method,
        args: request.args,
    };
    match manager.call(call, ctx.cancel.clone()).await {
        Ok(value) => OpOutcome::ok(&value),
        Err(error) => OpOutcome::from_resource_error(&error),
    }
}

/// Spawns the owner task for one operation. Returns the ledger id the guest
/// will correlate on.
pub fn spawn_operation(
    ledger: &Arc<Ledger>,
    ctx: OpContext,
    kind: &str,
    payload: String,
    completions: tokio::sync::mpsc::Sender<Completion>,
) -> Result<OpId, OpOutcome> {
    let future = start_builtin(kind, ctx.clone(), payload)?;
    let op = ctx.op;
    let world = ctx.world;
    let guard = OpGuard::new(Arc::clone(ledger), op);
    tokio::spawn(async move {
        let outcome = future.await;
        // The send fails only when the world's receiver is gone; the guard
        // then drops here and ownership still returns to baseline.
        let _ = completions
            .send(Completion {
                world,
                op,
                outcome,
                guard,
            })
            .await;
    });
    Ok(op)
}
