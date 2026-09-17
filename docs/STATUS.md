# Status

> Where the project is right now. Update this in the same change that moves it.

**Last updated:** 2026-09-17

## Current milestone

**D4 — Tasks + Explicit Ownership Transfer** (next). D0–D3 are done.

## Done

- **D0 foundation.** Rust workspace (`crates/usai-runtime`, `crates/usai-cli`), pnpm workspace (`packages/usai`, `packages/create-usai`, `examples/hello`, test fixtures), `make check` (fmt, clippy `-D warnings`, cargo test, tsc, node --test), GitHub Actions CI, pinned Rust 1.98.1 / Node 24 / pnpm 12.
- **D1 lifecycle core** (`usai-runtime`): immutable `ApplicationDefinition`; engine boundary + QuickJS-ng substrate (ADR-0015) + guest bridge ABI (`docs/GUEST-ABI.md`); `WorldDriver` with one identity-first completion gate; bounded ownership `Ledger` + gauges; host operations with independent owners; `ResourceManager`/`ResourceIdentity`/`cache.local`; hierarchical `Budget`; revisions `installed → active → draining → retired`. 17 acceptance tests in `tests/lifecycle.rs`.
- **D2 HTTP contract workload.** `usai` SDK (`defineApp`, `defineModule`, `http.*`, `http.raw`, `auth.*`, `errors`, `env`, `cache.local`, `task`/`cron`/`command`/`service` declarations, Standard Schema interop, manifest `describe`, in-world dispatcher). Runtime HTTP host: `matchit` router built once per revision, decode, JSON Schema validation **before** world creation (Zod 4 describes itself via `~standard.jsonSchema`), scalar coercion for string transports, admission, world, response contract, error mapping (C11), raw escape hatch, client-disconnect cancellation, deadline → 504. Build pipeline: esbuild via the SDK's `build/bundle.mjs` + manifest extraction in a capability-less world (ADR-0009). 13 acceptance tests in `tests/http.rs` against a real server and the real SDK.
- **D3 developer loop.** `usai build`, `usai run`, `usai dev` (watch → rebuild → install → activate → drain previous; failed builds keep the previous revision serving), `usai inspect` (+`--json`), `usai config` (effective values with sources; `usai.config.ts` evaluated declaratively). `examples/hello` runs.
- Design review closed 14 of 16 open questions as ADR-0001…0014; ADR-0015 records the engine decision.

## In progress

- nothing

## Next (D4 acceptance: HTTP request can dispatch a task and end safely; task gets a separate world; parent state does not leak; ownership transfer explicit and observable; dangling async work rejected/cancelled/diagnosed)

1. Runtime task subsystem: `task.invoke` op (owned, awaited child world) and `task.dispatch` op (ownership transferred to a runtime-owned task queue with its own budget; non-durable, ADR-0010).
2. Task world input `{kind:"task", input}` already supported by the SDK dispatcher; add runtime-side input validation from the manifest's `input` schema.
3. Observability: dispatch recorded in the parent's `WorkResult` (children), task outcomes logged.
4. Tests: HTTP → dispatch → task runs after the response; invoke → parent waits; parent globals invisible in task world; budgets; cancel semantics.

## Known gaps / debt

- The QuickJS substrate is native, not the Wasmtime pooling+COW representation the research measured (ADR-0015). Do not cite EXP-012B economics for it.
- Boundary contracts are validated twice when a JSON Schema exists (host before the world, provider inside it to obtain parsed values). Acceptable for v0; `GOAL.md` §13 asks to collapse this later.
- Auth resolvers run inside the world (after structural validation); `inspect` says so.
- `create-usai` is a placeholder; `examples/hello` is the onboarding path for now.
- No PostgreSQL provider yet (D6). No OpenAPI yet (D8).
- HTTP/1.1 only; no TLS termination (expected behind a proxy in v0).
