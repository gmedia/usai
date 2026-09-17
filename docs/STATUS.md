# Status

> Where the project is right now. Update this in the same change that moves it.

**Last updated:** 2026-09-17

## Current milestone

**D8 — API Metadata + OpenAPI** (next). D0–D7 are done.

## Done

- **D0 foundation.** Rust workspace (`crates/usai-runtime`, `crates/usai-cli`), pnpm workspace (`packages/usai`, `packages/create-usai`, `examples/hello`, test fixtures), `make check` (fmt, clippy `-D warnings`, cargo test, tsc, node --test), GitHub Actions CI, pinned Rust 1.98.1 / Node 24 / pnpm 12.
- **D1 lifecycle core** (`usai-runtime`): immutable `ApplicationDefinition`; engine boundary + QuickJS-ng substrate (ADR-0015) + guest bridge ABI (`docs/GUEST-ABI.md`); `WorldDriver` with one identity-first completion gate; bounded ownership `Ledger` + gauges; host operations with independent owners; `ResourceManager`/`ResourceIdentity`/`cache.local`; hierarchical `Budget`; revisions `installed → active → draining → retired`. 17 acceptance tests in `tests/lifecycle.rs`.
- **D2 HTTP contract workload.** `usai` SDK (`defineApp`, `defineModule`, `http.*`, `http.raw`, `auth.*`, `errors`, `env`, `cache.local`, `task`/`cron`/`command`/`service` declarations, Standard Schema interop, manifest `describe`, in-world dispatcher). Runtime HTTP host: `matchit` router built once per revision, decode, JSON Schema validation **before** world creation (Zod 4 describes itself via `~standard.jsonSchema`), scalar coercion for string transports, admission, world, response contract, error mapping (C11), raw escape hatch, client-disconnect cancellation, deadline → 504. Build pipeline: esbuild via the SDK's `build/bundle.mjs` + manifest extraction in a capability-less world (ADR-0009). 13 acceptance tests in `tests/http.rs` against a real server and the real SDK.
- **D3 developer loop.** `usai build`, `usai run`, `usai dev` (watch → rebuild → install → activate → drain previous; failed builds keep the previous revision serving), `usai inspect` (+`--json`), `usai config` (effective values with sources; `usai.config.ts` evaluated declaratively). `examples/hello` runs.
- **D4 tasks + explicit ownership transfer.** `ctx.tasks.invoke` = owned child world (cancelled with the parent, outcome returned as a contract); `ctx.tasks.dispatch` = ownership transferred to the runtime-owned, bounded, non-durable `TaskQueue` (ADR-0010; losses at shutdown are counted). Child records on `WorkResult`; dispatch is not detached work; draining waits for dispatched tasks; parent globals invisible in the child world.
- **D5 cron + commands.** Per-revision cron schedulers (croner, 5/6-field), fresh world per tick, overlap `skip`/`allow`, invalid schedules fail at install, schedulers stop at drain; `RuntimeConfig.cron_scheduler` for instances that must not tick. `Runtime::run_cron` / `run_command` / `run_task` for deterministic invocation; CLI `usai cron run`, `usai app <cmd> [args]`, `usai task run --input`. 9 acceptance tests in `tests/workloads.rs`.
- **D6 PostgreSQL capability.** `resource/postgres.rs` (tokio-postgres + deadpool): pool at runtime lifetime, one lease per operation, prepared statements cached per physical connection, JSON↔SQL typing by prepared-statement parameter types (int/float/numeric/bool/text/uuid/json/timestamps/date/arrays) and by column types. C5 paths: normal/SQL error → terminal → return; cooperative cancel → `CancelRequest` → await the original query's 57014 → return; abandonment without terminal proof → quarantine (removed from pool); session state reset on checkout (`pool.recycling: clean` default). Unreachable database fails activation. SDK `postgres("main", { urlEnv, pool })` + `query/one/execute`. 8 acceptance tests in `tests/postgres.rs` against a real server (`USAI_TEST_DATABASE_URL`, else a portable PostgreSQL 17 the tests download to `~/.cache/usai/postgresql`). `examples/postgres`.
- **D7 project model.** Module contributions (`migrations:`/`seeders:` globs, resources declared by several modules dedupe when identical and error when they conflict); migration discovery from `usai.config.ts` includes + module globs (root-relative, ordered by file name); `usai db migrate` applies each SQL file in its own transaction on a leased connection with a `usai_migrations` ledger row in the same transaction, refuses tampered applied files, never runs at startup; `usai db status`; seeders are `seeder(...)` default exports discovered by glob and run by `usai db seed [name]` as `command:seed:<name>` in a synthetic definition that borrows the app's resources and env; typed env validated for shape at activation (url/int/bool/enum) and delivered typed in `ctx.env`; `execute()` always injects the revision env. Fixture `tests/fixtures/project-app` uses colocated and centralized layouts at once. 3 acceptance tests in `tests/project.rs`.
- Design review closed 14 of 16 open questions as ADR-0001…0014; ADR-0015 records the engine decision.

## In progress

- nothing

## Next (D8 acceptance: docs match runtime behaviour; no duplicate endpoint-definition system; raw endpoints represented as opaque)

1. `openapi.rs`: OpenAPI 3.1 document generated from the ApplicationDefinition — paths from HTTP triggers, parameters from `params`/`query`/`headers` JSON Schema, request bodies, responses by status, declared errors as error responses, security schemes from auth declarations, raw endpoints as opaque operations.
2. `usai generate openapi [--out]`; dev server serves `/_usai/openapi.json` and a minimal docs page at `/_usai/docs`.
3. Tests: generated document validates structurally and reflects the fixture.

## Known gaps / debt

- The QuickJS substrate is native, not the Wasmtime pooling+COW representation the research measured (ADR-0015). Do not cite EXP-012B economics for it.
- Boundary contracts are validated twice when a JSON Schema exists (host before the world, provider inside it to obtain parsed values). Acceptable for v0; `GOAL.md` §13 asks to collapse this later.
- Auth resolvers run inside the world (after structural validation); `inspect` says so.
- `create-usai` is a placeholder; `examples/hello` is the onboarding path for now.
- No OpenAPI yet (D8). Module-level `migrations:`/`seeders:` globs are root-relative, not module-relative (the bundle has no source locations); documented on `defineModule`. PostgreSQL is `NoTls` only in v0 (expected behind a private network or a TLS proxy); no transactions across operations yet.
- HTTP/1.1 only; no TLS termination (expected behind a proxy in v0).
