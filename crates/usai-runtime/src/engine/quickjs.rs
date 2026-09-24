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

use super::{Compiled, Engine, EngineError, GUEST_BRIDGE, GuestState, HostBindings, WorldInstance};
use crate::definition::Code;

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

    async fn describe(
        &self,
        compiled: &Arc<dyn Compiled>,
    ) -> Result<serde_json::Value, EngineError> {
        self.eval_in(compiled, "(() => { const s = globalThis.__usai_sdk; const ns = globalThis.__usai_app_ns; const app = globalThis.__usai_app !== undefined ? globalThis.__usai_app : ns && ns.default; if (!s || typeof s.describe !== 'function') return null; return JSON.stringify(s.describe(app)); })()", "the application bundle did not register __usai_sdk.describe; is `usai` imported?").await
    }

    async fn export_default(
        &self,
        compiled: &Arc<dyn Compiled>,
    ) -> Result<serde_json::Value, EngineError> {
        self.eval_in(compiled, "(() => { const ns = globalThis.__usai_app_ns; const v = globalThis.__usai_app !== undefined ? globalThis.__usai_app : ns && ns.default; return v === undefined ? null : JSON.stringify(v); })()", "the module has no default export").await
    }

    async fn compile_code(&self, code: &Code) -> Result<Arc<dyn Compiled>, EngineError> {
        let (runtime, _) = new_runtime(&self.config).await?;
        let context = AsyncContext::full(&runtime)
            .await
            .map_err(|e| EngineError::Compile(e.to_string()))?;
        let source: Arc<str> = Arc::clone(&code.source);
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
                // The bundle is a global-style script (IIFE) that leaves the
                // application namespace on `__usai_app_ns`; the bridge reads
                // its default export. Nothing else to bind.
                let _ = module;
                Ok(())
            })
            .await?;
        Ok(Box::new(QuickJsWorld {
            _runtime: runtime,
            context,
            interrupt,
            phases: Vec::new(),
        }))
    }
}

impl QuickJsEngine {
    /// Evaluates `expression` (which must return a JSON string or null) in a
    /// capability-less world over the compiled module.
    async fn eval_in(
        &self,
        compiled: &Arc<dyn Compiled>,
        expression: &'static str,
        missing: &'static str,
    ) -> Result<serde_json::Value, EngineError> {
        let bindings: Arc<dyn HostBindings> = Arc::new(super::RefusingBindings);
        let mut world = self.instantiate(compiled, bindings).await?;
        let instance = world
            .as_any_mut()
            .downcast_mut::<QuickJsWorld>()
            .ok_or_else(|| EngineError::Guest("world is not a QuickJS world".into()))?;
        instance
            .context
            .with(move |ctx| {
                let result = ctx
                    .eval::<Option<String>, _>(expression)
                    .map_err(|e| EngineError::Guest(describe(&ctx, e)).or_declaration())?;
                match result {
                    None => Err(EngineError::Guest(missing.into())),
                    Some(json) => serde_json::from_str(&json)
                        .map_err(|e| EngineError::Guest(format!("result is not decodable: {e}"))),
                }
            })
            .await
    }
}

pub struct QuickJsWorld {
    _runtime: AsyncRuntime,
    context: AsyncContext,
    interrupt: Arc<AtomicBool>,
    phases: Vec<(&'static str, std::time::Duration)>,
}

impl QuickJsWorld {
    fn account(&mut self, name: &'static str, since: std::time::Instant) {
        if super::profiling() {
            let d = since.elapsed();
            match self.phases.iter_mut().find(|(k, _)| *k == name) {
                Some((_, total)) => *total += d,
                None => self.phases.push((name, d)),
            }
        }
    }
}

fn bridge<'js>(ctx: &Ctx<'js>) -> Result<Object<'js>, EngineError> {
    ctx.globals()
        .get::<_, Object>("__usai")
        .map_err(|e| EngineError::Guest(describe(ctx, e)))
}

#[async_trait]
impl WorldInstance for QuickJsWorld {
    async fn invoke(&mut self, index: usize, input_json: &str) -> Result<(), EngineError> {
        let t = std::time::Instant::now();
        let input = input_json.to_owned();
        let entropy = super::world_entropy();
        let r = self
            .context
            .with(move |ctx| {
                let b = bridge(&ctx)?;
                let seed: Function = b
                    .get("seed")
                    .map_err(|e| EngineError::Guest(describe(&ctx, e)))?;
                seed.call::<_, ()>((entropy, super::profiling()))
                    .map_err(|e| EngineError::Guest(describe(&ctx, e)))?;
                let invoke: Function = b
                    .get("invoke")
                    .map_err(|e| EngineError::Guest(describe(&ctx, e)))?;
                invoke
                    .call::<_, ()>((index as f64, input))
                    .map_err(|e| EngineError::Guest(describe(&ctx, e)))?;
                run_jobs(&ctx);
                Ok(())
            })
            .await;
        self.account("invoke", t);
        r
    }

    async fn deliver(&mut self, op: u64, ok: bool, payload: &str) -> Result<bool, EngineError> {
        let t = std::time::Instant::now();
        let payload = payload.to_owned();
        let r = self
            .context
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
            .await;
        self.account("deliver", t);
        r
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

    async fn stop(&mut self, reason: &str) -> Result<(), EngineError> {
        let reason = reason.to_owned();
        self.context
            .with(move |ctx| {
                let b = bridge(&ctx)?;
                let stop: Function = b
                    .get("stop")
                    .map_err(|e| EngineError::Guest(describe(&ctx, e)))?;
                stop.call::<_, ()>((reason,))
                    .map_err(|e| EngineError::Guest(describe(&ctx, e)))?;
                run_jobs(&ctx);
                Ok(())
            })
            .await
    }

    async fn state(&mut self) -> Result<GuestState, EngineError> {
        let t = std::time::Instant::now();
        let r = self
            .context
            .with(|ctx| {
                let b = bridge(&ctx)?;
                let state: Function = b
                    .get("state")
                    .map_err(|e| EngineError::Guest(describe(&ctx, e)))?;
                let json: String = state
                    .call(("",))
                    .map_err(|e| EngineError::Guest(describe(&ctx, e)))?;
                serde_json::from_str::<GuestState>(&json)
                    .map_err(|e| EngineError::Guest(format!("guest state is not decodable: {e}")))
            })
            .await;
        self.account("state", t);
        r
    }

    fn interrupter(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.interrupt)
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn phases(&self) -> Vec<(&'static str, std::time::Duration)> {
        self.phases.clone()
    }
}
