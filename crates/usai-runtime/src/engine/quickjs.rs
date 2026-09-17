//! QuickJS-ng execution substrate via `rquickjs` (ADR-0015).
//!
//! Definition lifetime: the application module is compiled to bytecode once.
//! World lifetime: a fresh QuickJS runtime + context, the guest bridge, the
//! module loaded from bytecode and evaluated to its baseline. Nothing is
//! shared between worlds except the immutable bytecode.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use async_trait::async_trait;
use rquickjs::{
    AsyncContext, AsyncRuntime, CatchResultExt, Ctx, Error as JsError, Function, Module, Object,
    Value, WriteOptions, context::EvalOptions,
};

use super::{
    Compiled, Engine, EngineError, GUEST_BRIDGE, HostBindings, Outcome, Pending, WorldInstance,
};
use crate::definition::ApplicationDefinition;

const MODULE_NAME: &str = "usai:app";

#[derive(Clone, Debug)]
pub struct QuickJsConfig {
    /// Per-world heap limit in bytes.
    pub memory_limit: usize,
    /// Per-world native stack limit in bytes.
    pub max_stack_size: usize,
}

impl Default for QuickJsConfig {
    fn default() -> Self {
        Self {
            memory_limit: 64 * 1024 * 1024,
            max_stack_size: 1024 * 1024,
        }
    }
}

pub struct QuickJsEngine {
    config: QuickJsConfig,
}

impl QuickJsEngine {
    pub fn new(config: QuickJsConfig) -> Arc<Self> {
        Arc::new(Self { config })
    }
}

struct Bytecode {
    bytes: Vec<u8>,
}

impl Compiled for Bytecode {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// Renders a thrown JS value (or a Rust-side error) as one line for
/// diagnostics. Never leaks into a client response by itself.
fn describe(ctx: &Ctx<'_>, error: JsError) -> String {
    match error {
        JsError::Exception => {
            let thrown = ctx.catch();
            if let Some(obj) = thrown.as_object() {
                let message: Option<String> = obj.get("message").ok();
                let name: Option<String> = obj.get("name").ok();
                let stack: Option<String> = obj.get("stack").ok();
                let mut out = format!(
                    "{}: {}",
                    name.unwrap_or_else(|| "Error".into()),
                    message.unwrap_or_default()
                );
                if let Some(stack) = stack
                    && !stack.is_empty()
                {
                    out.push('\n');
                    out.push_str(&stack);
                }
                out
            } else {
                format!("thrown: {thrown:?}")
            }
        }
        other => other.to_string(),
    }
}

fn run_jobs(ctx: &Ctx<'_>) {
    // A job that throws is reported through the promise it belongs to; the
    // bridge captures unhandled rejections of the outer handler promise.
    while ctx.execute_pending_job() {}
}

async fn new_runtime(
    config: &QuickJsConfig,
) -> Result<(AsyncRuntime, Arc<AtomicBool>), EngineError> {
    let runtime = AsyncRuntime::new().map_err(|e| EngineError::Instantiate(e.to_string()))?;
    runtime.set_memory_limit(config.memory_limit).await;
    runtime.set_max_stack_size(config.max_stack_size).await;
    let interrupt = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&interrupt);
    runtime
        .set_interrupt_handler(Some(Box::new(move || flag.load(Ordering::Relaxed))))
        .await;
    Ok((runtime, interrupt))
}

#[async_trait]
impl Engine for QuickJsEngine {
    fn name(&self) -> &'static str {
        "quickjs"
    }

    async fn compile(
        &self,
        definition: &ApplicationDefinition,
    ) -> Result<Arc<dyn Compiled>, EngineError> {
        let (runtime, _) = new_runtime(&self.config).await?;
        let context = AsyncContext::full(&runtime)
            .await
            .map_err(|e| EngineError::Compile(e.to_string()))?;
        let source: Arc<str> = Arc::clone(&definition.code().source);
        let bytes = context
            .with(move |ctx| {
                let module = Module::declare(ctx.clone(), MODULE_NAME, source.as_bytes().to_vec())
                    .map_err(|e| EngineError::Compile(describe(&ctx, e)))?;
                module
                    .write(WriteOptions::default())
                    .map_err(|e| EngineError::Compile(describe(&ctx, e)))
            })
            .await?;
        Ok(Arc::new(Bytecode { bytes }))
    }

    async fn instantiate(
        &self,
        compiled: &Arc<dyn Compiled>,
        bindings: Arc<dyn HostBindings>,
    ) -> Result<Box<dyn WorldInstance>, EngineError> {
        let bytecode = compiled
            .as_any()
            .downcast_ref::<Bytecode>()
            .ok_or_else(|| {
                EngineError::Instantiate("compiled form is not QuickJS bytecode".into())
            })?;
        let (runtime, interrupt) = new_runtime(&self.config).await?;
        let context = AsyncContext::full(&runtime)
            .await
            .map_err(|e| EngineError::Instantiate(e.to_string()))?;
        let bytes = bytecode.bytes.clone();
        context
            .with(move |ctx| -> Result<(), EngineError> {
                let fail = |ctx: &Ctx<'_>, e: JsError| EngineError::Instantiate(describe(ctx, e));
                let globals = ctx.globals();
                let b = Arc::clone(&bindings);
                globals
                    .set(
                        "__usai_host_start",
                        Function::new(ctx.clone(), move |kind: String, payload: String| -> f64 {
                            b.start(&kind, &payload) as f64
                        })
                        .map_err(|e| fail(&ctx, e))?,
                    )
                    .map_err(|e| fail(&ctx, e))?;
                let b = Arc::clone(&bindings);
                globals
                    .set(
                        "__usai_host_cancel",
                        Function::new(ctx.clone(), move |op: f64| b.cancel_op(op as u64))
                            .map_err(|e| fail(&ctx, e))?,
                    )
                    .map_err(|e| fail(&ctx, e))?;
                let b = Arc::clone(&bindings);
                globals
                    .set(
                        "__usai_host_log",
                        Function::new(ctx.clone(), move |level: String, message: String| {
                            b.log(&level, &message)
                        })
                        .map_err(|e| fail(&ctx, e))?,
                    )
                    .map_err(|e| fail(&ctx, e))?;

                let mut options = EvalOptions::default();
                options.global = true;
                options.strict = true;
                options.filename = Some("usai:bridge".into());
                ctx.eval_with_options::<(), _>(GUEST_BRIDGE, options)
                    .map_err(|e| fail(&ctx, e))?;

                // SAFETY: the bytes were produced by `Module::write` in this
                // same binary from the definition's source (`compile`).
                let module =
                    unsafe { Module::load(ctx.clone(), &bytes) }.map_err(|e| fail(&ctx, e))?;
                let (module, promise) = module.eval().map_err(|e| fail(&ctx, e))?;
                run_jobs(&ctx);
                promise.finish::<Value>().catch(&ctx).map_err(|e| {
                    EngineError::Instantiate(format!("module evaluation failed: {e}"))
                })?;
                let namespace = module.namespace().map_err(|e| fail(&ctx, e))?;
                let app: Value = namespace.get("default").map_err(|e| fail(&ctx, e))?;
                globals.set("__usai_app", app).map_err(|e| fail(&ctx, e))?;
                Ok(())
            })
            .await?;
        Ok(Box::new(QuickJsWorld {
            _runtime: runtime,
            context,
            interrupt,
        }))
    }
}

pub struct QuickJsWorld {
    _runtime: AsyncRuntime,
    context: AsyncContext,
    interrupt: Arc<AtomicBool>,
}

fn bridge<'js>(ctx: &Ctx<'js>) -> Result<Object<'js>, EngineError> {
    ctx.globals()
        .get::<_, Object>("__usai")
        .map_err(|e| EngineError::Guest(describe(ctx, e)))
}

#[async_trait]
impl WorldInstance for QuickJsWorld {
    async fn invoke(&mut self, index: usize, input_json: &str) -> Result<(), EngineError> {
        let input = input_json.to_owned();
        self.context
            .with(move |ctx| {
                let b = bridge(&ctx)?;
                let invoke: Function = b
                    .get("invoke")
                    .map_err(|e| EngineError::Guest(describe(&ctx, e)))?;
                invoke
                    .call::<_, ()>((index as f64, input))
                    .map_err(|e| EngineError::Guest(describe(&ctx, e)))?;
                run_jobs(&ctx);
                Ok(())
            })
            .await
    }

    async fn deliver(&mut self, op: u64, ok: bool, payload: &str) -> Result<bool, EngineError> {
        let payload = payload.to_owned();
        self.context
            .with(move |ctx| {
                let b = bridge(&ctx)?;
                let complete: Function = b
                    .get("complete")
                    .map_err(|e| EngineError::Guest(describe(&ctx, e)))?;
                let accepted: bool = complete
                    .call((op as f64, ok, payload))
                    .map_err(|e| EngineError::Guest(describe(&ctx, e)))?;
                run_jobs(&ctx);
                Ok(accepted)
            })
            .await
    }

    async fn cancel(&mut self, reason: &str) -> Result<(), EngineError> {
        let reason = reason.to_owned();
        self.context
            .with(move |ctx| {
                let b = bridge(&ctx)?;
                let cancel: Function = b
                    .get("cancel")
                    .map_err(|e| EngineError::Guest(describe(&ctx, e)))?;
                cancel
                    .call::<_, ()>((reason,))
                    .map_err(|e| EngineError::Guest(describe(&ctx, e)))?;
                run_jobs(&ctx);
                Ok(())
            })
            .await
    }

    async fn outcome(&mut self) -> Result<Option<Outcome>, EngineError> {
        self.context
            .with(|ctx| {
                let b = bridge(&ctx)?;
                let outcome: Function = b
                    .get("outcome")
                    .map_err(|e| EngineError::Guest(describe(&ctx, e)))?;
                let raw: Option<String> = outcome
                    .call(())
                    .map_err(|e| EngineError::Guest(describe(&ctx, e)))?;
                match raw {
                    None => Ok(None),
                    Some(json) => serde_json::from_str::<Outcome>(&json)
                        .map(Some)
                        .map_err(|e| EngineError::Guest(format!("outcome is not decodable: {e}"))),
                }
            })
            .await
    }

    async fn pending(&mut self) -> Result<Pending, EngineError> {
        self.context
            .with(|ctx| {
                let b = bridge(&ctx)?;
                let count: Function = b
                    .get("pendingCount")
                    .map_err(|e| EngineError::Guest(describe(&ctx, e)))?;
                let kinds: Function = b
                    .get("pendingKinds")
                    .map_err(|e| EngineError::Guest(describe(&ctx, e)))?;
                let count: f64 = count
                    .call(())
                    .map_err(|e| EngineError::Guest(describe(&ctx, e)))?;
                let kinds: Vec<String> = kinds
                    .call(())
                    .map_err(|e| EngineError::Guest(describe(&ctx, e)))?;
                Ok(Pending {
                    count: count as u32,
                    kinds,
                })
            })
            .await
    }

    fn interrupter(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.interrupt)
    }
}
