# Status

> Where the project is right now. Update this in the same change that moves it.

**Last updated:** 2026-09-17

## Current milestone

**D6 — PostgreSQL Capability** (next). D0–D5 are done.

## Done

- **D0 foundation.** Rust workspace (`crates/usai-runtime`, `crates/usai-cli`), pnpm workspace (`packages/usai`, `packages/create-usai`, `examples/hello`, test fixtures), `make check` (fmt, clippy `-D warnings`, cargo test, tsc, node --test), GitHub Actions CI, pinned Rust 1.98.1 / Node 24 / pnpm 12.
- **D1 lifecycle core** (`usai-runtime`): immutable `ApplicationDefinition`; engine boundary + QuickJS-ng substrate (ADR-0015) + guest bridge ABI (`docs/GUEST-ABI.md`); `WorldDriver` with one identity-first completion gate; bounded ownership `Ledger` + gauges; host operations with independent owners; `ResourceManager`/`ResourceIdentity`/`cache.local`; hierarchical `Budget`; revisions `installed → active → draining → retired`. 17 acceptance tests in `tests/lifecycle.rs`.
- **D2 HTTP contract workload.** `usai` SDK (`defineApp`, `defineModule`, `http.*`, `http.raw`, `auth.*`, `errors`, `env`, `cache.local`, `task`/`cron`/`command`/`service` declarations, Standard Schema interop, manifest `describe`, in-world dispatcher). Runtime HTTP host: `matchit` router built once per revision, decode, JSON Schema validation **before** world creation (Zod 4 describes itself via `~standard.jsonSchema`), scalar coercion for string transports, admission, world, response contract, error mapping (C11), raw escape hatch, client-disconnect cancellation, deadline → 504. Build pipeline: esbuild via the SDK's `build/bundle.mjs` + manifest extraction in a capability-less world (ADR-0009). 13 acceptance tests in `tests/http.rs` against a real server and the real SDK.
- **D3 developer loop.** `usai build`, `usai run`, `usai dev` (watch → rebuild → install → activate → drain previous; failed builds keep the previous revision serving), `usai inspect` (+`--json`), `usai config` (effective values with sources; `usai.config.ts` evaluated declaratively). `examples/hello` runs.
- **D4 tasks + explicit ownership transfer.** `ctx.tasks.invoke` = owned child world (cancelled with the parent, outcome returned as a contract); `ctx.tasks.dispatch` = ownership transferred to the runtime-owned, bounded, non-durable `TaskQueue` (ADR-0010; losses at shutdown are counted). Child records on `WorkResult`; dispatch is not detached work; draining waits for dispatched tasks; parent globals invisible in the child world.
- **D5 cron + commands.** Per-revision cron schedulers (croner, 5/6-field), fresh world per tick, overlap `skip`/`allow`, invalid schedules fail at install, schedulers stop at drain; `RuntimeConfig.cron_scheduler` for instances that must not tick. `Runtime::run_cron` / `run_command` / `run_task` for deterministic invocation; CLI `usai cron run`, `usai app <cmd> [args]`, `usai task run --input`. 9 acceptance tests in `tests/workloads.rs`.
- Design review closed 14 of 16 open questions as ADR-0001…0014; ADR-0015 records the engine decision.

## In progress

- nothing

## Next (D6 acceptance: connection reuse only after terminal knowledge; no connection-state leak across worlds; cancellation covered; abandoned-world behaviour covered; recovery request succeeds after abnormal cases)

1. `postgres` resource provider (`tokio-postgres` + `deadpool-postgres`): per-operation lease; normal/SQL-error → terminal → return; cooperative cancel → `CancelRequest` then await 57014 → return; hard abandonment → `Ambiguous` → quarantine (remove from pool); backend identity per physical connection.
2. SDK `postgres("main", { url: env ref })` declaration and in-world handle (`query`, `one`, `execute`, `transaction` later).
3. Tests against a docker PostgreSQL (gated on `USAI_TEST_DATABASE_URL`); all four C5 paths plus recovery.

## Known gaps / debt

- The QuickJS substrate is native, not the Wasmtime pooling+COW representation the research measured (ADR-0015). Do not cite EXP-012B economics for it.
- Boundary contracts are validated twice when a JSON Schema exists (host before the world, provider inside it to obtain parsed values). Acceptable for v0; `GOAL.md` §13 asks to collapse this later.
- Auth resolvers run inside the world (after structural validation); `inspect` says so.
- `create-usai` is a placeholder; `examples/hello` is the onboarding path for now.
- No PostgreSQL provider yet (D6). No OpenAPI yet (D8).
- HTTP/1.1 only; no TLS termination (expected behind a proxy in v0).
