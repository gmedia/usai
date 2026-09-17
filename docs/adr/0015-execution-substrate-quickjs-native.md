# ADR-0015: The bootstrap execution substrate is native QuickJS-ng (via rquickjs) behind an engine boundary

**Status:** accepted for functional scaffolding and as the reference/bootstrap engine; **not** accepted as the production economic substrate — superseded for production by ADR-0016
**Date:** 2026-09-17
**Closes:** —

## Context

The research lineage ran QuickJS-ng compiled to WebAssembly inside Wasmtime with pooling + copy-on-write and Wizer pre-initialization. That representation earned the R3 economics (EXP-012B). Reproducing it requires a patched Wasmtime source tree, WASI SDK, a custom quickjs-wasi async bridge, and Wizer — a toolchain the production repository does not yet own and that `GOAL.md` §49 says must never leak into the public model.

`AGENTS.md` §3 lists "introduce a new engine/runtime representation" as a stop-and-surface decision. This ADR is that surfacing.

## Decision

- The runtime core (definitions, revisions, worlds, ledger, resources, admission) depends only on the `Engine` / `WorldInstance` traits in `crates/usai-runtime/src/engine/mod.rs`.
- The v0 substrate is **QuickJS-ng linked natively through `rquickjs`**: a fresh QuickJS runtime + context per world; the application module compiled to bytecode once per definition and loaded per world; memory limit, stack limit, and a host-side interrupt watchdog per world.
- The same engine family as the research (QuickJS-ng) is kept deliberately so guest semantics match; only the physical representation differs.
- The Wasmtime pooling + COW representation remains the intended path for density/economics work. Adopting it is an engine implementation behind the same boundary, not a redesign, and is scheduled for D13 evidence gathering.

## Consequences

- Easier: no Wasm toolchain in D0–D12; fast iteration on semantics; the engine boundary is exercised from day one.
- Harder: per-world cost is a fresh QuickJS runtime plus bytecode load — cheaper than research's naive baseline (no re-parse) but without COW sharing. This is **not** the representation whose economics were measured; do not cite EXP-012B numbers for it.
- Harder: native QuickJS gives no memory-safety boundary beyond the process; consistent with ADR-0008 (one trust domain).
- Forbidden: exposing rquickjs types outside `engine/quickjs.rs`; letting any other module import `rquickjs`.
- Revisit: when D13 profiling shows world creation dominating, or when many-app density (research §11.3) becomes decision-relevant.

## Measured (2026-09-17, release, `tests/profile_bundles.rs`)

```text
engine floor (fresh runtime + context)   0.9 ms/world
SDK-only bundle (26 KB)                  1.0 ms/world
zod/mini bundle (53 KB)                  1.3 ms/world
zod bundle (765 KB)                      6.6 ms/world
```

The revisit trigger has fired: for a realistic application the per-world cost is dominated by re-evaluating the application module (schema-library initialization) in every fresh context, which this substrate cannot snapshot. The lifecycle model is not the cost; the representation is. The next substrate decision should be made with these numbers against a Wasmtime + pre-initialized image prototype behind the same `Engine` boundary (ADR-0016 when it exists).

## Alternatives considered

- **Wasmtime + QuickJS Wasm now.** Rejected for v0: toolchain cost blocks every other milestone.
- **V8 / deno_core.** Rejected: EXP-004B showed snapshots do not fix context cost; heavier dependency; different engine family from the evidence.
- **Boa / other Rust engines.** Rejected: ecosystem maturity and no path to the research representation.
