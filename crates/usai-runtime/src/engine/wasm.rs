//! Wasm execution substrate (ADR-0016): Wasmtime + the sealed QuickJS-ng
//! core + a Wizer pre-initialized application image + pooling allocator with
//! copy-on-write memory. The representation the research lineage measured.
//!
//! ```text
//! definition lifetime   core.wasm ──instantiate──▶ bridge + bundle evaluated ──Wizer──▶ image
//!                       image ──compile──▶ InstancePre (linker resolved, exports indexed)
//! world lifetime        InstancePre ──pooling slot + COW view of the image──▶ fresh world
//!                       (no JavaScript is re-evaluated: the initialized baseline is the image)
//! ```
//!
//! The guest ABI is unchanged (`docs/GUEST-ABI.md`); the bridge detects the
//! core's `__usai_test_op` and routes operations through it, and the host
//! answers the core's `usai_op_start` import (`guest/PROVENANCE.md`).

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use wasmtime::{
    Caller, Config, Enabled, Engine as WtEngine, Instance, InstanceAllocationStrategy, InstancePre,
    Linker, Memory, Module, ModuleExport, PoolingAllocationConfig, Store, TypedFunc,
    UpdateDeadline,
};
use wasmtime_wizer::{WasmtimeWizer, Wizer};

use super::{
    Compiled, Engine, EngineError, GUEST_BRIDGE, GuestState, HostBindings, RefusingBindings,
    WorldInstance,
};
use crate::definition::Code;

/// The sealed core (`guest/PROVENANCE.md`).
pub const CORE: &[u8] = include_bytes!("../../guest/quickjs-async.wasm");
pub const CORE_SHA256: &str = "91f178dff664c187c0c8aa9d83ca1ab2a67bad6c78c946aa1329031a02167502";

const MAX_PAYLOAD: usize = 8 * 1024 * 1024;

/// The vendored Wasmtime (`vendor/wasmtime/Cargo.toml`); part of the
/// precompiled image's fingerprint. Wasmtime verifies its own header on
/// deserialize as well; this only makes the mismatch a clean skip.
const WASMTIME_VERSION: &str = "48.0.2";

/// The core to run: the vendored one, or `USAI_WASM_CORE=<path>` for
/// controlled comparisons (e.g. the research `-Oz` core). Profiling only.
fn core_bytes() -> Result<std::borrow::Cow<'static, [u8]>, EngineError> {
    match std::env::var("USAI_WASM_CORE") {
        Ok(path) => {
            let bytes = std::fs::read(&path)
                .map_err(|e| EngineError::Compile(format!("USAI_WASM_CORE {path}: {e}")))?;
            tracing::warn!(path, bytes = bytes.len(), "using an alternative guest core");
            Ok(std::borrow::Cow::Owned(bytes))
        }
        Err(_) => Ok(std::borrow::Cow::Borrowed(CORE)),
    }
}
const EVAL_TYPE_GLOBAL: i32 = 0;
/// One epoch tick; the deadline callback checks the watchdog flag each tick.
const EPOCH_TICK: Duration = Duration::from_millis(10);

#[derive(Clone, Debug)]
pub struct WasmConfig {
    /// Worlds that can be live at once: pooling slots (memories, stacks, instances).
    pub capacity: u32,
    /// Per-world linear memory cap.
    pub max_memory_bytes: usize,
    /// Bytes of a recycled slot's memory kept resident instead of madvised
    /// away. With the pagemap scan this is the *budget* of dirty pages reset
    /// in place (the rest is decommitted and refaults); it must cover a
    /// request's heap growth, which is why it is generous. See
    /// `vendor/README.md` for the region-count patch that makes this work.
    pub linear_memory_keep_resident: usize,
    pub table_keep_resident: usize,
    /// Use the kernel's PAGEMAP_SCAN to reset only dirtied pages, when available.
    pub pagemap_scan: bool,
}

impl Default for WasmConfig {
    fn default() -> Self {
        Self {
            capacity: 256,
            max_memory_bytes: 64 * 1024 * 1024,
            linear_memory_keep_resident: 8 * 1024 * 1024,
            table_keep_resident: 64 * 1024,
            pagemap_scan: true,
        }
    }
}

/// Store data: what the host imports can reach while the guest runs.
struct HostData {
    bindings: Arc<dyn HostBindings>,
    memory: Option<Memory>,
    /// Core op id -> ledger op id, and back. Bounded by live operations.
    ledger_by_native: HashMap<u32, u64>,
    native_by_ledger: HashMap<u64, u32>,
    kind_by_native: HashMap<u32, String>,
    /// Completions to deliver once the current guest entry returns (a guest
    /// cannot be re-entered from inside a host import).
    deferred: Vec<(u32, u32, String)>,
    accepting: bool,
    op_starts: u64,
    /// The world's own zero for the monotonic clock. A world is a fresh
    /// context, so `performance.now()` starts near zero in it — and, unlike
    /// the wall clock, it cannot step backwards when the host's clock is
    /// corrected (this machine steps it by more than a second under load).
    monotonic_base: std::time::Instant,
}

impl HostData {
    fn new(bindings: Arc<dyn HostBindings>, build_time: bool) -> Self {
        Self {
            bindings,
            memory: None,
            ledger_by_native: HashMap::new(),
            native_by_ledger: HashMap::new(),
            kind_by_native: HashMap::new(),
            deferred: Vec::new(),
            accepting: !build_time,
            op_starts: 0,
            monotonic_base: std::time::Instant::now(),
        }
    }
}

fn read_bytes(caller: &Caller<'_, HostData>, ptr: i32, len: i32) -> Option<Vec<u8>> {
    if ptr < 0 || len < 0 || len as usize > MAX_PAYLOAD {
        return None;
    }
    let memory = caller.data().memory?;
    let mut out = vec![0u8; len as usize];
    memory.read(caller, ptr as usize, &mut out).ok()?;
    Some(out)
}

/// The host side of the core's import surface. Every operation the guest
/// starts arrives here as `kind\0payload`; control operations (`__cancel`,
/// `__log`) are answered synchronously by refusing the core's promise.
fn link(engine: &WtEngine) -> Result<Linker<HostData>, EngineError> {
    let mut linker = Linker::new(engine);
    let fail = |e: wasmtime::Error| EngineError::Instantiate(e.to_string());
    linker
        .func_wrap(
            "env",
            "usai_op_start",
            |mut caller: Caller<'_, HostData>, op_id: i32, _kind: i32, ptr: i32, len: i32| -> i32 {
                if op_id <= 0 {
                    return -1;
                }
                let Some(bytes) = read_bytes(&caller, ptr, len) else {
                    return -1;
                };
                let text = String::from_utf8_lossy(&bytes);
                let (kind, payload) = text.split_once('\0').unwrap_or((text.as_ref(), ""));
                let (kind, payload) = (kind.to_owned(), payload.to_owned());
                let data = caller.data_mut();
                match kind.as_str() {
                    "__log" => {
                        let (level, message) = payload
                            .split_once('\0')
                            .unwrap_or(("info", payload.as_str()));
                        data.bindings.log(level, message);
                        -1
                    }
                    "__cancel" => {
                        if let Some(native) = payload.trim().parse::<u32>().ok()
                            && let Some(ledger) = data.ledger_by_native.get(&native).copied()
                        {
                            data.bindings.cancel_op(ledger);
                            data.deferred.push((native, 2, "cancelled".into()));
                        }
                        -1
                    }
                    _ => {
                        // Only a real operation counts: the build phase reads
                        // this to refuse I/O in a declaration (ADR-0009), and
                        // `__log`/`__cancel` are control calls the host
                        // answers on the spot. Counting `__log` made a
                        // `console.log` at module scope fail the build with
                        // "declarations must not perform I/O", which is both
                        // true of the counter and wrong about logging.
                        data.op_starts += 1;
                        if !data.accepting {
                            return -1;
                        }
                        let ledger = data.bindings.start(&kind, &payload);
                        if ledger <= 0 {
                            return -1;
                        }
                        let native = op_id as u32;
                        data.ledger_by_native.insert(native, ledger as u64);
                        data.native_by_ledger.insert(ledger as u64, native);
                        data.kind_by_native.insert(native, kind);
                        0
                    }
                }
            },
        )
        .map_err(fail)?;
    linker
        .func_wrap(
            "env",
            "host_get_timezone_offset",
            |_: Caller<'_, HostData>, _hi: i32, _lo: i32| -> i32 { 0 },
        )
        .map_err(fail)?;
    linker
        .func_wrap("env", "host_interrupt", |_: Caller<'_, HostData>| -> i32 {
            0
        })
        .map_err(fail)?;
    linker
        .func_wrap(
            "env",
            "host_promise_rejection",
            |_: Caller<'_, HostData>, _p: i32, _r: i32, _h: i32| {},
        )
        .map_err(fail)?;
    linker
        .func_wrap(
            "env",
            "host_module_normalize",
            |_: Caller<'_, HostData>, _b: i32, _n: i32| -> i32 { 0 },
        )
        .map_err(fail)?;
    linker
        .func_wrap(
            "env",
            "host_module_load",
            |_: Caller<'_, HostData>, _n: i32, _l: i32| -> i32 { 0 },
        )
        .map_err(fail)?;
    linker
        .func_wrap(
            "env",
            "host_call",
            |_: Caller<'_, HostData>, _a: i32, _b: i32, _c: i32, _d: i32, _e: i32| -> i32 { 0 },
        )
        .map_err(fail)?;
    let wasi = "wasi_snapshot_preview1";
    linker
        .func_wrap(
            wasi,
            "clock_time_get",
            |mut caller: Caller<'_, HostData>, id: i32, _precision: i64, out: i32| -> i32 {
                // WASI clock ids: 0 realtime, 1 monotonic (2/3 are CPU-time
                // clocks; the monotonic answer is the honest one for them).
                let now = if id == 0 {
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_nanos() as u64)
                        .unwrap_or(0)
                } else {
                    caller.data().monotonic_base.elapsed().as_nanos() as u64
                };
                let Some(memory) = caller.data().memory else {
                    return 1;
                };
                if memory
                    .write(&mut caller, out as usize, &now.to_le_bytes())
                    .is_err()
                {
                    1
                } else {
                    0
                }
            },
        )
        .map_err(fail)?;
    linker
        .func_wrap(
            wasi,
            "random_get",
            |mut caller: Caller<'_, HostData>, ptr: i32, len: i32| -> i32 {
                if ptr < 0 || len < 0 {
                    return 1;
                }
                let mut bytes = vec![0u8; len as usize];
                if getrandom::fill(&mut bytes).is_err() {
                    return 1;
                }
                let Some(memory) = caller.data().memory else {
                    return 1;
                };
                if memory.write(&mut caller, ptr as usize, &bytes).is_err() {
                    1
                } else {
                    0
                }
            },
        )
        .map_err(fail)?;
    linker
        .func_wrap(
            wasi,
            "fd_close",
            |_: Caller<'_, HostData>, _fd: i32| -> i32 { 0 },
        )
        .map_err(fail)?;
    linker
        .func_wrap(
            wasi,
            "fd_fdstat_get",
            |_: Caller<'_, HostData>, _fd: i32, _out: i32| -> i32 { 0 },
        )
        .map_err(fail)?;
    linker
        .func_wrap(
            wasi,
            "fd_seek",
            |_: Caller<'_, HostData>, _fd: i32, _o: i64, _w: i32, _out: i32| -> i32 { 0 },
        )
        .map_err(fail)?;
    linker
        .func_wrap(
            wasi,
            "fd_write",
            |mut caller: Caller<'_, HostData>, fd: i32, iovs: i32, count: i32, out: i32| -> i32 {
                let Some(memory) = caller.data().memory else {
                    return 1;
                };
                let mut written = 0usize;
                let mut text = Vec::new();
                for index in 0..count.max(0) {
                    let mut entry = [0u8; 8];
                    if memory
                        .read(&caller, (iovs + index * 8) as usize, &mut entry)
                        .is_err()
                    {
                        return 1;
                    }
                    let ptr =
                        u32::from_le_bytes(entry[0..4].try_into().expect("four bytes")) as usize;
                    let len =
                        u32::from_le_bytes(entry[4..8].try_into().expect("four bytes")) as usize;
                    let mut chunk = vec![0u8; len.min(64 * 1024)];
                    if memory.read(&caller, ptr, &mut chunk).is_ok() {
                        text.extend_from_slice(&chunk);
                    }
                    written += len;
                }
                if !text.is_empty() {
                    let message = String::from_utf8_lossy(&text);
                    caller
                        .data()
                        .bindings
                        .log(if fd == 2 { "error" } else { "info" }, message.trim_end());
                }
                if memory
                    .write(&mut caller, out as usize, &(written as u32).to_le_bytes())
                    .is_err()
                {
                    1
                } else {
                    0
                }
            },
        )
        .map_err(fail)?;
    Ok(linker)
}

const EXPORT_NAMES: [&str; 21] = [
    "memory",
    "qjs_run_gc",
    "qjs_usai_inbuf",
    "qjs_usai_enter",
    "qjs_usai_settle",
    "wasm_malloc",
    "wasm_free",
    "qjs_init",
    "qjs_eval",
    "qjs_is_exception",
    "qjs_get_exception",
    "qjs_get_string",
    "qjs_free_cstring",
    "qjs_free_value",
    "qjs_is_job_pending",
    "qjs_execute_pending_job",
    "qjs_reseed_math_random",
    "qjs_usai_install_async_bridge",
    "qjs_usai_pending_op_count",
    "qjs_usai_op_complete",
    "qjs_usai_bridge_install_count",
];

/// Definition-lifetime: the pre-initialized image compiled for the runtime
/// engine, its linker resolved once, its exports indexed once.
struct Image {
    pre: InstancePre<HostData>,
    exports: Vec<ModuleExport>,
    image_sha256: String,
    /// The compiled module, kept so `precompile` can serialize it.
    module: Module,
}

impl Compiled for Image {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// Compiled images alive right now: each holds a deserialized module and
/// its memory image. A retired revision must bring this down again — the
/// revision-churn campaign watches it.
pub static IMAGES_LIVE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

impl Drop for Image {
    fn drop(&mut self) {
        IMAGES_LIVE.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
        tracing::debug!(image_sha256 = %self.image_sha256, "application image dropped");
    }
}

pub struct WasmEngine {
    config: WasmConfig,
    runtime: WtEngine,
    /// Default allocator, used only to build images.
    builder: WtEngine,
    _ticker: std::thread::JoinHandle<()>,
    worlds_created: AtomicU64,
}

fn base_config() -> Config {
    let mut config = Config::new();
    // Profiling knobs (not application configuration).
    let epoch = std::env::var("USAI_WASM_EPOCH").as_deref() != Ok("0");
    config.epoch_interruption(epoch);
    // `USAI_WASM_PERFMAP=1` writes /tmp/perf-<pid>.map so `perf report`
    // names the core's functions inside JIT code (Linux only; engineering).
    if std::env::var("USAI_WASM_PERFMAP").as_deref() == Ok("1") {
        config.profiler(wasmtime::ProfilingStrategy::PerfMap);
    }
    if let Ok(level) = std::env::var("USAI_WASM_OPT") {
        config.cranelift_opt_level(match level.as_str() {
            "none" => wasmtime::OptLevel::None,
            "size" => wasmtime::OptLevel::SpeedAndSize,
            _ => wasmtime::OptLevel::Speed,
        });
    }
    // Compiled artifacts are cached on disk keyed by their bytes and this
    // configuration: the core compiles once per machine, an image once per
    // application revision. `USAI_COMPILE_CACHE=0` turns it off (a read-only
    // container serving a precompiled artifact has nothing to cache).
    if std::env::var("USAI_COMPILE_CACHE").as_deref() != Ok("0") {
        match wasmtime::Cache::from_file(None) {
            Ok(cache) => {
                config.cache(Some(cache));
            }
            Err(e) => tracing::warn!(error = %e, "wasmtime compilation cache unavailable"),
        }
    }
    config.memory_init_cow(true);
    config.memory_may_move(true);
    config.async_stack_size(2 * 1024 * 1024);
    config.max_wasm_stack(512 * 1024);
    config
}

impl WasmEngine {
    pub fn new(config: WasmConfig) -> Result<Arc<Self>, EngineError> {
        let mut runtime_config = base_config();
        let mut pooling = PoolingAllocationConfig::new();
        let totals = config.capacity.max(1);
        pooling.total_component_instances(totals);
        pooling.total_core_instances(totals);
        pooling.total_memories(totals);
        pooling.total_tables(totals);
        pooling.total_stacks(totals);
        pooling.max_core_instances_per_component(1);
        pooling.max_memories_per_component(1);
        pooling.max_tables_per_component(1);
        pooling.max_memory_size(config.max_memory_bytes);
        pooling.linear_memory_keep_resident(config.linear_memory_keep_resident);
        pooling.table_keep_resident(config.table_keep_resident);
        // Every world resets its slot fully anyway, so slot↔module affinity
        // buys nothing here — but Wasmtime's default keeps up to 100 unused
        // warm slots and prefers *cold* slots for a new module, so revision
        // churn touched ever more slots and their keep-resident pages until
        // the container's memory limit (P6 campaign: OOM after ~270
        // replacements at 512 MB). With 0 the slots ever used equal the peak
        // concurrency: RSS ≈ live worlds × keep_resident, whatever the churn.
        pooling.max_unused_warm_slots(0);
        let pagemap = PoolingAllocationConfig::is_pagemap_scan_available();
        if config.pagemap_scan && pagemap {
            pooling.pagemap_scan(Enabled::Yes);
        }
        tracing::info!(
            pagemap_scan = pagemap && config.pagemap_scan,
            keep_resident = config.linear_memory_keep_resident,
            capacity = totals,
            "wasm engine"
        );
        runtime_config.allocation_strategy(InstanceAllocationStrategy::Pooling(pooling));
        let runtime =
            WtEngine::new(&runtime_config).map_err(|e| EngineError::Instantiate(e.to_string()))?;
        let builder =
            WtEngine::new(&base_config()).map_err(|e| EngineError::Instantiate(e.to_string()))?;
        // Epoch ticks let the deadline callback observe the watchdog flag.
        let ticker = {
            let engines = [runtime.clone(), builder.clone()];
            std::thread::Builder::new()
                .name("usai-epoch".into())
                .spawn(move || {
                    loop {
                        // A tick only matters to a live world's deadline
                        // callback; with none live the thread parks (an
                        // idle process wakes for nothing).
                        crate::idle::wait_until_active();
                        std::thread::sleep(EPOCH_TICK);
                        for engine in &engines {
                            engine.increment_epoch();
                        }
                    }
                })
                .map_err(|e| EngineError::Instantiate(e.to_string()))?
        };
        Ok(Arc::new(Self {
            config,
            runtime,
            builder,
            _ticker: ticker,
            worlds_created: AtomicU64::new(0),
        }))
    }

    pub fn config(&self) -> &WasmConfig {
        &self.config
    }

    /// Builds the pre-initialized image: core + bridge + application module
    /// evaluated to quiescence, then snapshotted.
    async fn build_image(&self, code: &Code) -> Result<Vec<u8>, EngineError> {
        let compile = |e: wasmtime::Error| EngineError::Compile(e.to_string());
        let wizer = Wizer::new();
        let core = core_bytes()?;
        let (context, instrumented) = wizer.instrument(&core).map_err(compile)?;
        let module = Module::new(&self.builder, &instrumented).map_err(compile)?;
        let linker = link(&self.builder)?;
        let bindings: Arc<dyn HostBindings> = Arc::new(RefusingBindings);
        let mut store = Store::new(&self.builder, HostData::new(bindings, true));
        store.set_epoch_deadline(u64::MAX / 2);
        let instance = linker.instantiate(&mut store, &module).map_err(compile)?;
        let memory = instance
            .get_memory(&mut store, "memory")
            .ok_or_else(|| EngineError::Compile("core has no memory export".into()))?;
        store.data_mut().memory = Some(memory);
        let mut guest = Guest::new(&mut store, instance, &module, memory)?;
        if let Ok(init) = instance.get_typed_func::<(), ()>(&mut store, "_initialize") {
            init.call(&mut store, ()).map_err(compile)?;
        }
        let status = guest.call1::<(), i32>(&mut store, "qjs_init", ()).await?;
        if status != 0 {
            return Err(EngineError::Compile(format!("qjs_init returned {status}")));
        }
        let status = guest
            .call1::<(), i32>(&mut store, "qjs_usai_install_async_bridge", ())
            .await?;
        if status != 0 {
            return Err(EngineError::Compile(format!(
                "bridge install returned {status}"
            )));
        }
        guest
            .eval(&mut store, GUEST_BRIDGE, "usai:bridge")
            .await
            .map_err(|e| EngineError::Compile(format!("bridge: {e}")))?;
        guest
            .eval(&mut store, &code.source, "usai:app")
            .await
            .map_err(|e| {
                EngineError::Compile(format!("application module evaluation failed: {e}"))
            })?;
        guest.run_jobs(&mut store).await?;
        // Warm the validators so their lazily built state is in the image
        // (every world would otherwise rebuild it; measured 1.6 ms for zod).
        let warmed = guest
            .eval(
                &mut store,
                "(function () { const sdk = globalThis.__usai_sdk; const app = globalThis.__usai_app ?? (typeof __usai_app_ns === 'object' ? __usai_app_ns.default : undefined); return String(sdk && typeof sdk.warm === 'function' && app ? sdk.warm(app) : 0); })()",
                "usai:warm",
            )
            .await
            .map_err(|e| EngineError::Compile(format!("validator warm-up failed: {e}")))?;
        guest.run_jobs(&mut store).await?;
        tracing::debug!(parses = warmed, "validators warmed before snapshot");
        // Collect the warm-up's garbage before the snapshot. QuickJS triggers
        // a full collection when its allocation count crosses a threshold set
        // relative to the live size at the last collection; a snapshot taken
        // just under that threshold made every world's first parse pay the
        // whole cycle (measured 0.4–0.8 ms per request on every world).
        guest.call1::<(), ()>(&mut store, "qjs_run_gc", ()).await?;
        let pending = guest
            .call1::<(), i32>(&mut store, "qjs_usai_pending_op_count", ())
            .await?;
        if pending != 0 || store.data().op_starts != 0 {
            return Err(EngineError::Compile(format!(
                "the application started {} operation(s) while being defined; declarations must not perform I/O (ADR-0009)",
                store.data().op_starts
            )));
        }
        let image = wizer
            .snapshot(
                &context,
                &mut WasmtimeWizer {
                    store: &mut store,
                    instance,
                },
            )
            .await
            .map_err(compile)?;
        Ok(image)
    }
}

/// Typed access to the core's exports, resolved lazily per world (C13) and
/// cached: a request enters the guest a few hundred times, and each entry
/// must not pay an export lookup, a type check, or a fiber switch.
struct Guest {
    instance: Instance,
    memory: Memory,
    exports: Vec<ModuleExport>,
    funcs: HashMap<&'static str, wasmtime::Func>,
    /// The request path's exports, resolved once per world and kept as
    /// typed handles: no export lookup, no type check per call (the R3
    /// lesson — "typed exports resolved once").
    hot: HotExports,
    /// The guest-owned input buffer (`qjs_usai_inbuf`) and its capacity;
    /// grown on demand, never freed by the host.
    inbuf: (i32, usize),
}

#[derive(Default)]
struct HotExports {
    inbuf: Option<TypedFunc<i32, i32>>,
    enter: Option<TypedFunc<(i32, i32, i32, i32), i64>>,
    settle: Option<TypedFunc<(), i64>>,
    complete: Option<TypedFunc<(i32, i32, i32, i32), i32>>,
}

macro_rules! hot {
    ($self:ident, $store:ident, $field:ident, $name:literal, $p:ty, $r:ty) => {{
        match &$self.hot.$field {
            Some(f) => f.clone(),
            None => {
                let f = $self.func::<$p, $r>($store, $name)?;
                $self.hot.$field = Some(f.clone());
                f
            }
        }
    }};
}

impl Guest {
    fn new(
        store: &mut Store<HostData>,
        instance: Instance,
        module: &Module,
        memory: Memory,
    ) -> Result<Self, EngineError> {
        let _ = store;
        let exports = EXPORT_NAMES
            .iter()
            .map(|name| {
                module
                    .get_export_index(name)
                    .ok_or_else(|| EngineError::Compile(format!("core lacks export {name}")))
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            instance,
            memory,
            exports,
            funcs: HashMap::new(),
            hot: HotExports::default(),
            inbuf: (0, 0),
        })
    }

    fn from_exports(instance: Instance, memory: Memory, exports: Vec<ModuleExport>) -> Self {
        Self {
            instance,
            memory,
            exports,
            funcs: HashMap::new(),
            hot: HotExports::default(),
            inbuf: (0, 0),
        }
    }

    fn func<P, R>(
        &mut self,
        store: &mut Store<HostData>,
        name: &'static str,
    ) -> Result<TypedFunc<P, R>, EngineError>
    where
        P: wasmtime::WasmParams,
        R: wasmtime::WasmResults,
    {
        let func = match self.funcs.get(name) {
            Some(f) => *f,
            None => {
                let index = EXPORT_NAMES
                    .iter()
                    .position(|n| *n == name)
                    .ok_or_else(|| EngineError::Guest(format!("unknown export {name}")))?;
                let export = self.exports[index];
                let f = self
                    .instance
                    .get_module_export(&mut *store, &export)
                    .and_then(|e| e.into_func())
                    .ok_or_else(|| {
                        EngineError::Guest(format!("export {name} is not a function"))
                    })?;
                self.funcs.insert(name, f);
                f
            }
        };
        func.typed(&*store)
            .map_err(|e| EngineError::Guest(format!("export {name}: {e}")))
    }

    /// A synchronous entry: guest work is CPU-bound and never yields, so a
    /// fiber per call would only add switches.
    async fn call1<P, R>(
        &mut self,
        store: &mut Store<HostData>,
        name: &'static str,
        params: P,
    ) -> Result<R, EngineError>
    where
        P: wasmtime::WasmParams + Sync,
        R: wasmtime::WasmResults + Sync,
    {
        let f = self.func::<P, R>(store, name)?;
        f.call(store, params)
            .map_err(|e| EngineError::Guest(format!("{name}: {e:#}")))
    }

    async fn alloc(
        &mut self,
        store: &mut Store<HostData>,
        bytes: &[u8],
        nul: bool,
    ) -> Result<i32, EngineError> {
        let total = bytes.len() + usize::from(nul);
        let ptr = self
            .call1::<i32, i32>(store, "wasm_malloc", total.max(1) as i32)
            .await?;
        if ptr <= 0 {
            return Err(EngineError::Guest("wasm_malloc returned null".into()));
        }
        self.memory
            .write(&mut *store, ptr as usize, bytes)
            .map_err(|e| EngineError::Guest(e.to_string()))?;
        if nul {
            self.memory
                .write(&mut *store, ptr as usize + bytes.len(), &[0])
                .map_err(|e| EngineError::Guest(e.to_string()))?;
        }
        Ok(ptr)
    }

    async fn free(&mut self, store: &mut Store<HostData>, ptr: i32) -> Result<(), EngineError> {
        self.call1::<i32, ()>(store, "wasm_free", ptr).await
    }

    fn read_cstring(&self, store: &Store<HostData>, ptr: i32) -> Result<String, EngineError> {
        if ptr <= 0 {
            return Err(EngineError::Guest("guest returned a null string".into()));
        }
        let data = self.memory.data(store);
        let tail = data
            .get(ptr as usize..)
            .ok_or_else(|| EngineError::Guest("string pointer out of bounds".into()))?;
        let len = tail
            .iter()
            .position(|b| *b == 0)
            .ok_or_else(|| EngineError::Guest("unterminated string".into()))?;
        Ok(String::from_utf8_lossy(&tail[..len]).into_owned())
    }

    async fn string_of(
        &mut self,
        store: &mut Store<HostData>,
        value: i32,
    ) -> Result<String, EngineError> {
        let ptr = self
            .call1::<i32, i32>(store, "qjs_get_string", value)
            .await?;
        let text = self.read_cstring(store, ptr)?;
        self.call1::<i32, ()>(store, "qjs_free_cstring", ptr)
            .await?;
        Ok(text)
    }

    /// Writes `bytes` into the guest-owned input buffer and returns its
    /// address: one typed call when the buffer must grow, none otherwise.
    fn stage(&mut self, store: &mut Store<HostData>, bytes: &[u8]) -> Result<i32, EngineError> {
        if bytes.len() > self.inbuf.1 || self.inbuf.0 == 0 {
            let f = hot!(self, store, inbuf, "qjs_usai_inbuf", i32, i32);
            let ptr = f
                .call(&mut *store, bytes.len().max(1) as i32)
                .map_err(|e| EngineError::Guest(format!("qjs_usai_inbuf: {e:#}")))?;
            if ptr <= 0 {
                return Err(EngineError::Guest("guest input buffer unavailable".into()));
            }
            self.inbuf = (ptr, bytes.len().max(4096));
        }
        self.memory
            .write(&mut *store, self.inbuf.0 as usize, bytes)
            .map_err(|e| EngineError::Guest(e.to_string()))?;
        Ok(self.inbuf.0)
    }

    /// Decodes a `(len << 32) | ptr` result (bit 63: exception text) from
    /// guest memory; the core keeps the text until its next result.
    fn owned_text(&self, store: &Store<HostData>, packed: i64) -> Result<String, EngineError> {
        let packed = packed as u64;
        let exception = packed >> 63 == 1;
        let len = ((packed >> 32) & 0x7fff_ffff) as usize;
        let ptr = (packed & 0xffff_ffff) as usize;
        let data = self.memory.data(store);
        let bytes = data
            .get(ptr..ptr + len)
            .ok_or_else(|| EngineError::Guest("result text out of bounds".into()))?;
        let text = String::from_utf8_lossy(bytes).into_owned();
        if exception {
            Err(EngineError::Guest(text))
        } else {
            Ok(text)
        }
    }

    /// Calls `__usai[name](arg)` through the core's direct-call export
    /// (ADR-0018): the argument staged in the guest-owned buffer as
    /// `name\0arg`, one typed call, the result read straight from memory.
    /// No script is parsed or compiled, nothing is allocated per call on
    /// either side; this is the request hot path.
    async fn call(
        &mut self,
        store: &mut Store<HostData>,
        name: &str,
        arg: &str,
    ) -> Result<String, EngineError> {
        let mut buffer = Vec::with_capacity(name.len() + 1 + arg.len());
        buffer.extend_from_slice(name.as_bytes());
        buffer.push(0);
        buffer.extend_from_slice(arg.as_bytes());
        let ptr = self.stage(store, &buffer)?;
        let f = hot!(
            self,
            store,
            enter,
            "qjs_usai_enter",
            (i32, i32, i32, i32),
            i64
        );
        let packed = f
            .call(
                &mut *store,
                (
                    ptr,
                    name.len() as i32,
                    ptr + name.len() as i32 + 1,
                    arg.len() as i32,
                ),
            )
            .map_err(|e| EngineError::Guest(format!("{name}: {e:#}")))?;
        self.owned_text(store, packed)
    }

    /// Runs the job queue to quiescence inside the core and returns
    /// `__usai.state()`: one call where the driver's loop used to make one
    /// per job plus one for the state.
    async fn settle(&mut self, store: &mut Store<HostData>) -> Result<GuestState, EngineError> {
        let f = hot!(self, store, settle, "qjs_usai_settle", (), i64);
        let packed = f
            .call(&mut *store, ())
            .map_err(|e| EngineError::Guest(format!("settle: {e:#}")))?;
        let text = self.owned_text(store, packed)?;
        serde_json::from_str::<GuestState>(&text)
            .map_err(|e| EngineError::Guest(format!("guest state is not decodable: {e}")))
    }

    /// A heap JSValue from `qjs_eval`: the exception as an
    /// error, otherwise the value rendered as a string; freed either way.
    async fn result_text(
        &mut self,
        store: &mut Store<HostData>,
        value: i32,
    ) -> Result<String, EngineError> {
        let is_exception = self
            .call1::<i32, i32>(store, "qjs_is_exception", value)
            .await?
            != 0;
        if is_exception {
            self.call1::<i32, ()>(store, "qjs_free_value", value)
                .await?;
            let exception = self
                .call1::<(), i32>(store, "qjs_get_exception", ())
                .await?;
            let text = self.string_of(store, exception).await?;
            self.call1::<i32, ()>(store, "qjs_free_value", exception)
                .await?;
            return Err(EngineError::Guest(text));
        }
        let text = self.string_of(store, value).await?;
        self.call1::<i32, ()>(store, "qjs_free_value", value)
            .await?;
        Ok(text)
    }

    /// Evaluates a global script and returns its result rendered as a string.
    async fn eval(
        &mut self,
        store: &mut Store<HostData>,
        code: &str,
        filename: &str,
    ) -> Result<String, EngineError> {
        let code_ptr = self.alloc(store, code.as_bytes(), true).await?;
        let name_ptr = self.alloc(store, filename.as_bytes(), true).await?;
        let value = self
            .call1::<(i32, i32, i32, i32), i32>(
                store,
                "qjs_eval",
                (code_ptr, code.len() as i32, name_ptr, EVAL_TYPE_GLOBAL),
            )
            .await;
        self.free(store, code_ptr).await?;
        self.free(store, name_ptr).await?;
        self.result_text(store, value?).await
    }

    async fn run_jobs(&mut self, store: &mut Store<HostData>) -> Result<(), EngineError> {
        for _ in 0..100_000 {
            if self
                .call1::<(), i32>(store, "qjs_is_job_pending", ())
                .await?
                == 0
            {
                return Ok(());
            }
            // A job that throws reports through the promise it belongs to.
            let _ = self
                .call1::<(), i32>(store, "qjs_execute_pending_job", ())
                .await?;
        }
        Err(EngineError::Guest("job queue did not quiesce".into()))
    }

    async fn complete(
        &mut self,
        store: &mut Store<HostData>,
        native: u32,
        status: u32,
        payload: &str,
    ) -> Result<i32, EngineError> {
        let ptr = self.stage(store, payload.as_bytes())?;
        let f = hot!(
            self,
            store,
            complete,
            "qjs_usai_op_complete",
            (i32, i32, i32, i32),
            i32
        );
        f.call(
            &mut *store,
            (native as i32, status as i32, ptr, payload.len() as i32),
        )
        .map_err(|e| EngineError::Guest(format!("qjs_usai_op_complete: {e:#}")))
    }
}

impl WasmEngine {
    fn image_from_module(
        &self,
        module: Module,
        image_sha256: String,
    ) -> Result<Arc<dyn Compiled>, EngineError> {
        let exports = EXPORT_NAMES
            .iter()
            .map(|name| {
                module
                    .get_export_index(name)
                    .ok_or_else(|| EngineError::Compile(format!("image lacks export {name}")))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let pre = link(&self.runtime)?
            .instantiate_pre(&module)
            .map_err(|e| EngineError::Compile(e.to_string()))?;
        IMAGES_LIVE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Ok(Arc::new(Image {
            pre,
            exports,
            image_sha256,
            module,
        }))
    }
}

#[async_trait]
impl Engine for WasmEngine {
    fn name(&self) -> &'static str {
        "wasm"
    }

    async fn compile_code(&self, code: &Code) -> Result<Arc<dyn Compiled>, EngineError> {
        let image = self.build_image(code).await?;
        let image_sha256 = {
            use sha2::Digest as _;
            hex::encode(sha2::Sha256::digest(&image))
        };
        let t = std::time::Instant::now();
        let module =
            Module::new(&self.runtime, &image).map_err(|e| EngineError::Compile(e.to_string()))?;
        tracing::info!(
            image_sha256,
            bytes = image.len(),
            compile_ms = t.elapsed().as_millis() as u64,
            "application image built"
        );
        self.image_from_module(module, image_sha256)
    }

    fn fingerprint(&self) -> String {
        // What a serialized module depends on: Wasmtime's build (it checks
        // its own header too), the guest core, the bridge evaluated into the
        // image (a runtime that grew a bridge function must not load an
        // image without it), and the host target.
        use sha2::Digest;
        format!(
            "wasmtime={};core={};bridge={};target={}-{}",
            WASMTIME_VERSION,
            CORE_SHA256,
            &hex::encode(sha2::Sha256::digest(GUEST_BRIDGE.as_bytes()))[..16],
            std::env::consts::ARCH,
            std::env::consts::OS
        )
    }

    fn precompile(&self, compiled: &Arc<dyn Compiled>) -> Option<Vec<u8>> {
        let image = compiled.as_any().downcast_ref::<Image>()?;
        image.module.serialize().ok()
    }

    async fn load_precompiled(
        &self,
        _code: &Code,
        bytes: &[u8],
    ) -> Result<Arc<dyn Compiled>, EngineError> {
        let t = std::time::Instant::now();
        // SAFETY: `bytes` is native code produced by `Module::serialize` on
        // a compatible Wasmtime (its header is verified here); it comes from
        // the artifact directory, which is the deployment's trust boundary
        // (docs/THREAT-MODEL.md).
        let module = unsafe { Module::deserialize(&self.runtime, bytes) }
            .map_err(|e| EngineError::Compile(format!("precompiled image: {e}")))?;
        let image_sha256 = {
            use sha2::Digest as _;
            hex::encode(sha2::Sha256::digest(bytes))
        };
        tracing::info!(
            bytes = bytes.len(),
            load_ms = t.elapsed().as_millis() as u64,
            "precompiled application image loaded"
        );
        self.image_from_module(module, image_sha256)
    }

    async fn instantiate(
        &self,
        compiled: &Arc<dyn Compiled>,
        bindings: Arc<dyn HostBindings>,
    ) -> Result<Box<dyn WorldInstance>, EngineError> {
        let image = compiled
            .as_any()
            .downcast_ref::<Image>()
            .ok_or_else(|| EngineError::Instantiate("compiled form is not a Wasm image".into()))?;
        let t0 = std::time::Instant::now();
        let mut store = Store::new(&self.runtime, HostData::new(bindings, false));
        let interrupt = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&interrupt);
        store.set_epoch_deadline(1);
        store.epoch_deadline_callback(move |_| {
            if flag.load(Ordering::Relaxed) {
                Err(wasmtime::Error::msg(
                    "guest exceeded the synchronous CPU slice",
                ))
            } else {
                Ok(UpdateDeadline::Continue(1))
            }
        });
        let t1 = std::time::Instant::now();
        let instance = image.pre.instantiate(&mut store).map_err(|e| {
            if e.to_string().contains("pool") || e.to_string().contains("capacity") {
                EngineError::Capacity
            } else {
                EngineError::Instantiate(e.to_string())
            }
        })?;
        let memory = instance
            .get_module_export(&mut store, &image.exports[0])
            .and_then(|e| e.into_memory())
            .ok_or_else(|| EngineError::Instantiate("image has no memory".into()))?;
        store.data_mut().memory = Some(memory);
        let t2 = std::time::Instant::now();
        let mut guest = Guest::from_exports(instance, memory, image.exports.clone());
        let t3 = std::time::Instant::now();
        let mut seed = [0u8; 8];
        let _ = getrandom::fill(&mut seed);
        guest
            .call1::<i64, ()>(
                &mut store,
                "qjs_reseed_math_random",
                i64::from_le_bytes(seed),
            )
            .await?;
        self.worlds_created.fetch_add(1, Ordering::Relaxed);
        tracing::debug!(
            store_ms = (t1 - t0).as_secs_f64() * 1000.0,
            instantiate_and_seed_ms = t1.elapsed().as_secs_f64() * 1000.0,
            memory_bytes = memory.data_size(&store),
            "wasm world instantiate"
        );
        let _ = &image.image_sha256;
        let phases = if super::profiling() {
            vec![
                ("instantiate.store", t1 - t0),
                ("instantiate.instance", t2 - t1),
                ("instantiate.exports", t3 - t2),
                ("instantiate.seed", t3.elapsed()),
            ]
        } else {
            Vec::new()
        };
        Ok(Box::new(WasmWorld {
            store,
            guest,
            interrupt,
            phases,
            settled: None,
        }))
    }

    async fn describe(
        &self,
        compiled: &Arc<dyn Compiled>,
    ) -> Result<serde_json::Value, EngineError> {
        self.eval_in(compiled, "(() => { const s = globalThis.__usai_sdk; const ns = globalThis.__usai_app_ns; const app = globalThis.__usai_app !== undefined ? globalThis.__usai_app : ns && ns.default; if (!s || typeof s.describe !== 'function') return ''; return JSON.stringify(s.describe(app)); })()", "the application bundle did not register __usai_sdk.describe; is `usai` imported?").await
    }

    async fn export_default(
        &self,
        compiled: &Arc<dyn Compiled>,
    ) -> Result<serde_json::Value, EngineError> {
        self.eval_in(compiled, "(() => { const ns = globalThis.__usai_app_ns; const v = globalThis.__usai_app !== undefined ? globalThis.__usai_app : ns && ns.default; return v === undefined ? '' : JSON.stringify(v); })()", "the module has no default export").await
    }
}

impl WasmEngine {
    async fn eval_in(
        &self,
        compiled: &Arc<dyn Compiled>,
        expression: &str,
        missing: &str,
    ) -> Result<serde_json::Value, EngineError> {
        let bindings: Arc<dyn HostBindings> = Arc::new(RefusingBindings);
        let mut world = self.instantiate(compiled, bindings).await?;
        let world = world
            .as_any_mut()
            .downcast_mut::<WasmWorld>()
            .ok_or_else(|| EngineError::Guest("not a wasm world".into()))?;
        let text = world
            .guest
            .eval(&mut world.store, expression, "usai:eval")
            .await?;
        if text.is_empty() {
            return Err(EngineError::Guest(missing.into()));
        }
        serde_json::from_str(&text)
            .map_err(|e| EngineError::Guest(format!("result is not decodable: {e}")))
    }
}

pub struct WasmWorld {
    store: Store<HostData>,
    guest: Guest,
    interrupt: Arc<AtomicBool>,
    phases: Vec<(&'static str, Duration)>,
    /// The state `qjs_usai_settle` returned after the last guest activity;
    /// valid until the next call into the guest, so `state()` costs nothing.
    settled: Option<GuestState>,
}

impl WasmWorld {
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

impl WasmWorld {
    /// Runs the job queue to quiescence and keeps the resulting state;
    /// completions queued by host imports during the run are delivered and
    /// the queue drained again until none appear.
    async fn settle(&mut self, phase: &'static str) -> Result<(), EngineError> {
        loop {
            let t = std::time::Instant::now();
            let state = self.guest.settle(&mut self.store).await?;
            self.account(phase, t);
            let deferred = std::mem::take(&mut self.store.data_mut().deferred);
            if deferred.is_empty() {
                self.settled = Some(state);
                return Ok(());
            }
            for (native, status, payload) in deferred {
                self.forget(native);
                self.guest
                    .complete(&mut self.store, native, status, &payload)
                    .await?;
            }
        }
    }

    fn forget(&mut self, native: u32) {
        let data = self.store.data_mut();
        if let Some(ledger) = data.ledger_by_native.remove(&native) {
            data.native_by_ledger.remove(&ledger);
        }
        data.kind_by_native.remove(&native);
    }
}

#[async_trait]
impl WorldInstance for WasmWorld {
    async fn invoke(&mut self, index: usize, input_json: &str) -> Result<(), EngineError> {
        let t = std::time::Instant::now();
        let payload = format!(
            "{}\u{1f}{}\u{1f}{index}\u{1f}{input_json}",
            super::world_entropy(),
            u8::from(super::profiling()),
        );
        self.settled = None;
        self.guest.call(&mut self.store, "entry", &payload).await?;
        self.account("invoke.entry", t);
        self.settle("invoke.settle").await
    }

    async fn deliver(&mut self, op: u64, ok: bool, payload: &str) -> Result<bool, EngineError> {
        let Some(native) = self.store.data().native_by_ledger.get(&op).copied() else {
            return Ok(false);
        };
        self.forget(native);
        self.settled = None;
        let t = std::time::Instant::now();
        let status = self
            .guest
            .complete(&mut self.store, native, if ok { 0 } else { 1 }, payload)
            .await?;
        self.account("deliver.complete", t);
        self.settle("deliver.settle").await?;
        Ok(status == 0)
    }

    async fn cancel(&mut self, reason: &str) -> Result<(), EngineError> {
        self.store.data_mut().accepting = false;
        self.settled = None;
        self.guest.call(&mut self.store, "cancel", reason).await?;
        let outstanding: Vec<u32> = self.store.data().ledger_by_native.keys().copied().collect();
        for native in outstanding {
            self.forget(native);
            let _ = self
                .guest
                .complete(&mut self.store, native, 2, reason)
                .await?;
        }
        self.settle("cancel.settle").await
    }

    async fn stop(&mut self, reason: &str) -> Result<(), EngineError> {
        self.settled = None;
        self.guest.call(&mut self.store, "stop", reason).await?;
        let timers: Vec<(u32, u64)> = self
            .store
            .data()
            .kind_by_native
            .iter()
            .filter(|(_, kind)| kind.as_str() == "timer")
            .filter_map(|(native, _)| {
                self.store
                    .data()
                    .ledger_by_native
                    .get(native)
                    .map(|ledger| (*native, *ledger))
            })
            .collect();
        for (native, ledger) in timers {
            self.store.data().bindings.cancel_op(ledger);
            self.forget(native);
            let _ = self.guest.complete(&mut self.store, native, 0, "").await?;
        }
        self.settle("stop.settle").await
    }

    async fn state(&mut self) -> Result<GuestState, EngineError> {
        // Nothing changes in the guest without a host call, so the state the
        // last settle returned is the state; the guest is asked only when
        // no activity has produced one yet.
        if let Some(state) = &self.settled {
            return Ok(state.clone());
        }
        self.settle("state").await?;
        Ok(self.settled.clone().expect("settle keeps the state"))
    }

    fn interrupter(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.interrupt)
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn phases(&self) -> Vec<(&'static str, Duration)> {
        self.phases.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_vendored_core_matches_its_provenance() {
        use sha2::Digest as _;
        assert_eq!(hex::encode(sha2::Sha256::digest(CORE)), CORE_SHA256);
    }

    /// Two worlds in one pooling slot: the second must see the image and
    /// zeros where the first grew, whatever the first did to its memory.
    /// `dirty` is applied to the first world's whole memory (image + 64
    /// grown pages); `pageout` asks the kernel to page it out first.
    async fn assert_slot_is_fresh_after(
        dirty: impl Fn(&mut [u8]),
        pageout: bool,
        pagemap_scan: bool,
    ) {
        let engine = WasmEngine::new(WasmConfig {
            capacity: 1,
            pagemap_scan,
            ..WasmConfig::default()
        })
        .unwrap();
        let code = Code::new("globalThis.__usai_app = { workloads: [] };");
        let compiled = engine.compile_code(&code).await.unwrap();
        let bindings: Arc<dyn HostBindings> = Arc::new(RefusingBindings);

        // Reference contents from two pristine worlds; the bytes that differ
        // between them are per-world by design (the Math.random seed) and
        // are excluded from the comparison.
        let snapshot = |world: &mut Box<dyn WorldInstance>| {
            let w = world.as_any_mut().downcast_mut::<WasmWorld>().unwrap();
            let len = w.guest.memory.data_size(&w.store);
            w.guest.memory.data(&w.store)[..len].to_vec()
        };
        let mut world = engine
            .instantiate(&compiled, Arc::clone(&bindings))
            .await
            .unwrap();
        let pristine = snapshot(&mut world);
        drop(world);
        let image_len = pristine.len();
        // Four more pristine worlds: a seed byte that coincides by chance in
        // one pair differs in another, and the whole 64-byte window around
        // any differing byte is excluded so neighbouring state words are too.
        let mut per_world = std::collections::HashSet::new();
        for _ in 0..4 {
            let mut world = engine
                .instantiate(&compiled, Arc::clone(&bindings))
                .await
                .unwrap();
            let other = snapshot(&mut world);
            drop(world);
            assert_eq!(other.len(), image_len);
            for i in (0..image_len).filter(|i| pristine[*i] != other[*i]) {
                let window = i / 64 * 64;
                per_world.extend(window..(window + 64).min(image_len));
            }
        }
        assert!(
            per_world.len() < 1024,
            "unexpectedly many per-world bytes: {}",
            per_world.len()
        );

        // World 1 dirties memory (image and 64 grown pages), then maybe asks
        // the kernel to page it out (needs swap to actually happen; without
        // it the test still checks the ordinary reset).
        let mut world = engine
            .instantiate(&compiled, Arc::clone(&bindings))
            .await
            .unwrap();
        let w = world.as_any_mut().downcast_mut::<WasmWorld>().unwrap();
        w.guest.memory.grow(&mut w.store, 64).unwrap();
        let grown_len = w.guest.memory.data_size(&w.store);
        dirty(w.guest.memory.data_mut(&mut w.store));
        if pageout {
            let base = w.guest.memory.data_ptr(&w.store);
            // SAFETY: the range is this instance's linear memory, which stays
            // mapped for the instance's lifetime; MADV_PAGEOUT only affects
            // residency, never contents.
            let rc = unsafe { libc::madvise(base.cast(), grown_len, libc::MADV_PAGEOUT) };
            assert_eq!(rc, 0, "madvise: {}", std::io::Error::last_os_error());
        }
        drop(world);

        // World 2 must see the image, then zeros where world 1 grew.
        let mut world = engine
            .instantiate(&compiled, Arc::clone(&bindings))
            .await
            .unwrap();
        let w = world.as_any_mut().downcast_mut::<WasmWorld>().unwrap();
        assert_eq!(
            w.guest.memory.data_size(&w.store),
            image_len,
            "size is the image's again"
        );
        let seen = &w.guest.memory.data(&w.store)[..image_len];
        let first_diff =
            (0..image_len).find(|i| seen[*i] != pristine[*i] && !per_world.contains(i));
        assert_eq!(
            first_diff,
            None,
            "image page {:?} carried the previous world's bytes",
            first_diff.map(|i| i / 4096)
        );
        w.guest.memory.grow(&mut w.store, 64).unwrap();
        let tail = &w.guest.memory.data(&w.store)[image_len..grown_len];
        let dirty = tail.iter().position(|b| *b != 0);
        assert_eq!(
            dirty,
            None,
            "grown page {:?} carried the previous world's bytes",
            dirty.map(|i| i / 4096)
        );
    }

    fn scribble_everything(data: &mut [u8]) {
        for (i, b) in data.iter_mut().enumerate() {
            *b = (i % 251) as u8 ^ 0x5a;
        }
    }

    /// One byte every other page: hundreds of disjoint dirty regions, far
    /// more than the scan's per-call buffer, so the traversal must resume.
    fn scribble_fragmented(data: &mut [u8]) {
        for page in (0..data.len() / 4096).step_by(2) {
            data[page * 4096 + 17] = 0xa5;
        }
    }

    /// Freshness across slot reuse when the previous world's dirty pages were
    /// paged out: the reset must not require a page to be resident to reset
    /// it (`vendor/wasmtime-pagemap-reset.patch`).
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_reused_slot_is_fresh_even_after_its_pages_were_paged_out() {
        assert_slot_is_fresh_after(scribble_everything, true, true).await;
    }

    /// Fragmented dirty sets are reset completely: the scan resumes from
    /// `walk_end` instead of stopping at a fixed region count.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_reused_slot_is_fresh_after_fragmented_writes() {
        assert_slot_is_fresh_after(scribble_fragmented, false, true).await;
        assert_slot_is_fresh_after(scribble_fragmented, true, true).await;
    }

    /// The memcpy reset path (no pagemap scan) keeps the same property.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_reused_slot_is_fresh_without_the_pagemap_scan() {
        assert_slot_is_fresh_after(scribble_everything, true, false).await;
        assert_slot_is_fresh_after(scribble_fragmented, false, false).await;
    }
}
