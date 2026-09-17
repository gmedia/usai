# ADR-0016: Production execution substrate is Wasmtime + a pre-initialized QuickJS-ng image with pooling and copy-on-write memory

**Status:** accepted (v0) — economics measured on one machine, see below; production evidence still required
**Date:** 2026-09-18
**Closes:** the revisit trigger of ADR-0015

## Context

ADR-0015's native QuickJS substrate re-evaluates the application module in every fresh context: 6.6 ms/world for a Zod-using bundle, structural to that representation. The research lineage removed exactly this cost with a Wizer pre-initialized image and Wasmtime's pooling allocator + copy-on-write memory (EXP-011B, EXP-012B). The runtime core was written against an `Engine` boundary so the representation could change without touching semantics.

The research repository holds everything needed: the sealed QuickJS-ng + WASI + operation-bridge core, its fully pinned build (quickjs-wasi `54c4d2d`, quickjs-ng `65641a0`, WASI SDK 32, three patches totalling 296 lines), and the engine configuration. Upstream Wasmtime 48 ships `pagemap_scan`, `memory_init_cow`, keep-resident tuning and `wasmtime-wizer`; no patched Wasmtime is needed any more.

## Decision

- `engine/wasm.rs` is a second `Engine` implementation: definition lifetime = instantiate the core, evaluate the guest bridge and the application bundle to quiescence, snapshot with Wizer into an image, compile it once (`InstancePre`, exports indexed); world lifetime = a pooling slot with a copy-on-write view of the image. **No JavaScript is evaluated to create a world.**
- The guest ABI is unchanged. The bridge detects the core's `__usai_test_op` and routes operations through it; the host answers `usai_op_start`. Control operations (`__cancel`, `__log`) are refused synchronously by the core so the bridge stays sequential with the core's op-id counter.
- The core is built **from pinned sources in this repository** (`crates/usai-runtime/guest/build.sh`, patches vendored). `OPT=-Oz` reproduces the research core byte for byte (`6b33cb45…`); the production core is built with `-O3` (`c4e58003…`). A test refuses a core whose digest does not match.
- The application bundle is emitted as a global-style script (`__usai_app_ns`) so one artifact serves both substrates.
- `wasm` is the default engine; `quickjs` stays selectable (`--engine quickjs`, `USAI_ENGINE`) as the bootstrap/reference substrate and runs a subset of the suite in CI.
- Every acceptance suite (71 tests: lifecycle, HTTP, workloads, connection, project, control, hardening, PostgreSQL) passes on the Wasm engine.

## Measured (2026-09-18, release, this WSL2 machine — engineering numbers, not canonical)

Per-world instantiate + drop (`tests/profile_bundles.rs`):

```text
                     native QuickJS     Wasm image
SDK-only bundle        1.1 ms           0.01 ms
zod/mini               1.5 ms           0.01 ms
zod (806 KB)           7.2 ms           0.03 ms
```

Hello request through `Runtime::invoke`, both engines in one process, `-O3` core:

```text
                     native QuickJS     Wasm image
create + retire       7–18 ms*          0.55 ms
world run             1.2 ms            4.8 ms
```

\* the machine's speed varied 2× during the session; the ratio held.

Attribution of the Wasm world run: a pure-JS CPU loop runs 2.3× slower than native (`-O3` core; it was 3.3× with the research's `-Oz` core), and each request incurs ~200 minor page faults regardless of keep-resident/pagemap settings — on WSL2 (Hyper-V) minor faults are expensive. The fault source (likely heap growth beyond the image) is the next item to measure **on a real Linux VM**, where the research's economics were established.

## Consequences

- Easier: per-world cost is flat with respect to application size; the initialized baseline is physically shared and semantically fresh — the property the thesis is about.
- Harder: guest execution runs in Cranelift-compiled QuickJS (slower than native by a constant factor); tooling needs the compilation cache (`~/.cache/wasmtime`) to keep dev/test fast.
- Forbidden: citing these numbers as production evidence; they are one machine, one workload.
- Revisit: after measuring on the research VM with the bundle matrix (engine floor / SDK / zod-mini / zod / representative CRUD) and the metrics listed in `docs/STATUS.md`.

## Alternatives considered

- **Keep native QuickJS and reuse a runtime across worlds.** Rejected: contexts in one runtime share a heap; freshness would become a claim to audit rather than a property.
- **Rebuild QuickJS-Wasm with a new bridge from scratch.** Rejected: the sealed core and its pinned recipe already exist and reproduce.
