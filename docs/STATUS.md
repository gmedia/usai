# Status

> Where the project is right now. Update this in the same change that moves it.

**Last updated:** 2026-09-18

## Where things stand

```text
D0–D15                      FIRST IMPLEMENTATION PRESENT (each has acceptance tests)
Product breadth             ESTABLISHED — stop broadening; depth now
Milestone acceptance        AUDITED — docs/ACCEPTANCE-AUDIT.md: all D0–D15 items ✓ or ◐, no ✗
Production substrate        Wasm image + pooling/COW (ADR-0016); attributed and fixed on the research VM
                            (hello 1.01 ms p50, 13.6k req/s at c=16, 0 faults); 1 h soak in progress
Developer preview           READY TO TAG — release workflow in place, PG TLS done; artifact byte format not final
Production ready            NO
```

Current phase (after the 2026-09-18 review): **depth, not breadth.**

```text
execution substrate recovery   ← ADR-0016 first pass done; measure on a real VM next
end-to-end correctness audit
realistic application
performance / soak / multi-core
alpha
```

## Measured — substrate (2026-09-18, release; engineering numbers)

Full ledger in `docs/measurements/2026-09-18-execution-path-attribution.md`
(research VM, 16 × Xeon E5-2680 v4). After P1 attribution and P2 fixes:

```text
hello, usai bench, VM          P1 (aded35c)              P2 (ff2688f)
c=1                            369 req/s, p50 2.62 ms     658 req/s, p50 1.52 ms
c=16                           2 497 req/s, p50 6.24     9 763 req/s, p50 1.58
c=64                           2 471 req/s, p50 25.6     9 788 req/s, p50 6.45
minor faults / request         95–315                    0
```

What P1 found and P2 fixed: zod's lazy schema initialization was paid by
every fresh world (1.64 ms → prepared before the snapshot, callback-free);
Wasmtime's slot reset stopped its dirty-page scan at 32 regions and
decommitted the rest, so every world refaulted its heap and the
`madvise`/`mprotect` storm capped multi-core scaling with TLB-shootdown IPIs
(vendored one-line patch, 0 faults, ×3.9 at c=16); zero-delay timers cost a
1 ms timer-wheel tick (yield); the watchdog handshake cost 4 context switches
per request (atomic slots + ticker). The world's remaining 1.05 ms on the VM
is accounted: 0.46 validation + handler, 0.24 eval floor, 0.13 reset, 0.18
bookkeeping, 0.02 create.

## Done

- **D0 foundation.** Rust workspace (`crates/usai-runtime`, `crates/usai-cli`), pnpm workspace (`packages/usai`, `packages/create-usai`, `examples/hello`, test fixtures), `make check` (fmt, clippy `-D warnings`, cargo test, tsc, node --test), GitHub Actions CI, pinned Rust 1.98.1 / Node 24 / pnpm 12.
- **D1 lifecycle core** (`usai-runtime`): immutable `ApplicationDefinition`; engine boundary + QuickJS-ng substrate (ADR-0015) + guest bridge ABI (`docs/GUEST-ABI.md`); `WorldDriver` with one identity-first completion gate; bounded ownership `Ledger` + gauges; host operations with independent owners; `ResourceManager`/`ResourceIdentity`/`cache.local`; hierarchical `Budget`; revisions `installed → active → draining → retired`. 17 acceptance tests in `tests/lifecycle.rs`.
- **D2 HTTP contract workload.** `usai` SDK (`defineApp`, `defineModule`, `http.*`, `http.raw`, `auth.*`, `errors`, `env`, `cache.local`, `task`/`cron`/`command`/`service` declarations, Standard Schema interop, manifest `describe`, in-world dispatcher). Runtime HTTP host: `matchit` router built once per revision, decode, JSON Schema validation **before** world creation (Zod 4 describes itself via `~standard.jsonSchema`), scalar coercion for string transports, admission, world, response contract, error mapping (C11), raw escape hatch, client-disconnect cancellation, deadline → 504. Build pipeline: esbuild via the SDK's `build/bundle.mjs` + manifest extraction in a capability-less world (ADR-0009). 13 acceptance tests in `tests/http.rs` against a real server and the real SDK.
- **D3 developer loop.** `usai build`, `usai run`, `usai dev` (watch → rebuild → install → activate → drain previous; failed builds keep the previous revision serving), `usai inspect` (+`--json`), `usai config` (effective values with sources; `usai.config.ts` evaluated declaratively). `examples/hello` runs.
- **D4 tasks + explicit ownership transfer.** `ctx.tasks.invoke` = owned child world (cancelled with the parent, outcome returned as a contract); `ctx.tasks.dispatch` = ownership transferred to the runtime-owned, bounded, non-durable `TaskQueue` (ADR-0010; losses at shutdown are counted). Child records on `WorkResult`; dispatch is not detached work; draining waits for dispatched tasks; parent globals invisible in the child world.
- **D5 cron + commands.** Per-revision cron schedulers (croner, 5/6-field), fresh world per tick, overlap `skip`/`allow`, invalid schedules fail at install, schedulers stop at drain; `RuntimeConfig.cron_scheduler` for instances that must not tick. `Runtime::run_cron` / `run_command` / `run_task` for deterministic invocation; CLI `usai cron run`, `usai app <cmd> [args]`, `usai task run --input`. 9 acceptance tests in `tests/workloads.rs`.
- **D6 PostgreSQL capability.** `resource/postgres.rs` (tokio-postgres + deadpool): pool at runtime lifetime, one lease per operation, prepared statements cached per physical connection, JSON↔SQL typing by prepared-statement parameter types (int/float/numeric/bool/text/uuid/json/timestamps/date/arrays) and by column types. C5 paths: normal/SQL error → terminal → return; cooperative cancel → `CancelRequest` → await the original query's 57014 → return; abandonment without terminal proof → quarantine (removed from pool); session state reset on checkout (`pool.recycling: clean` default). Unreachable database fails activation. SDK `postgres("main", { urlEnv, pool })` + `query/one/execute`. 8 acceptance tests in `tests/postgres.rs` against a real server (`USAI_TEST_DATABASE_URL`, else a portable PostgreSQL 18 the tests download to `~/.cache/usai/postgresql`). `examples/postgres`.
- **D7 project model.** Module contributions (`migrations:`/`seeders:` globs, resources declared by several modules dedupe when identical and error when they conflict); migration discovery from `usai.config.ts` includes + module globs (root-relative, ordered by file name); `usai db migrate` applies each SQL file in its own transaction on a leased connection with a `usai_migrations` ledger row in the same transaction, refuses tampered applied files, never runs at startup; `usai db status`; seeders are `seeder(...)` default exports discovered by glob and run by `usai db seed [name]` as `command:seed:<name>` in a synthetic definition that borrows the app's resources and env; typed env validated for shape at activation (url/int/bool/enum) and delivered typed in `ctx.env`; `execute()` always injects the revision env. Fixture `tests/fixtures/project-app` uses colocated and centralized layouts at once. 3 acceptance tests in `tests/project.rs`.
- **D8 API metadata + OpenAPI.** `openapi.rs` generates OpenAPI 3.1 from the ApplicationDefinition: paths from HTTP triggers, path/query/header parameters from the extracted JSON Schemas, request bodies, responses by declared status, declared errors and the standard error envelope, security schemes from auth declarations, module names as tags; raw endpoints are opaque (`x-usai-raw`), in-world-only contracts are flagged (`x-usai-validated-in-world`). `usai generate openapi [--out]`; the dev server serves `/_usai/openapi.json` and a dependency-free `/_usai/docs`. 1 acceptance test (served document == generated document; not served on a production host).
- **D9 service workload.** `service("name", handler)` starts one persistent world per revision at activation (`workloads/services.rs` supervisor); state persists because the service is alive; finite worlds cannot see it. Drain stops services first: graceful (`__usai.stop` — `ctx.signal` fires, pending `sleep`s resolve, the loop exits and the handler's return is the service's outcome), hard cancel after `drain_timeout`. Service state in `RuntimeStatus.revisions[].services` and the `usai dev` banner. No restart policy yet (D13). 1 acceptance test.
- **D10 queue / message workload.** `queue.consume("topic", { message, concurrency, retry, database }, handler)` + `ctx.queue.publish(topic, message, { delayMs })`. the workload interface is substrate-independent (topic, message contract, concurrency, retry); the v0 *substrate* is a PostgreSQL table (`usai_queue`) claimed with `FOR UPDATE SKIP LOCKED` — persistent consumer loops per revision (`workloads/queue.rs`), fresh world per message, concurrency from the declaration, message JSON Schema validated before the world exists (invalid → dead, no world), explicit retry with fixed/exponential backoff and a dead-letter state (ADR-0014; delivery is at-least-once and says so), consumers stop at drain; `RuntimeConfig.queue_consumers`. 1 acceptance test (publish from HTTP world → four messages processed once, retry-then-success across three fresh worlds, dead letter, invalid message never gets a world).
- **D11 WebSocket + stream workloads.** Streams: `http.stream(path, { params, query, auth }, (ctx, stream) => …)` — the response commits at the first `stream.start`/`send`/`event` (SSE helper) and the body ends when the handler returns (`headers sent != work complete`); a handler that never sends is an ordinary response; a client disconnect cancels the world through the body's drop guard; drain stops the loop gracefully. Sockets: `socket(path, { incoming, outgoing }, { open, message, close })` — HTTP upgrade (hyper `with_upgrades` + tungstenite), one world per connection, frames delivered as host completions (`socket.recv`), `ctx.state` connection-local, contract violations reported to the client without closing, application or client close runs `close`, drain sends 1012 and runs `close`. Per-revision `connections_stop`. Server rewritten with per-connection tasks + `TaskTracker` drain. 5 acceptance tests in `tests/connection.rs`.
- **D12 observability + graph.** `observability.rs`: per-world trace record at `debug` (world, workload, revision, termination, outcome, duration, completions, children, violations) with a disabled fast path (`tracing::enabled!` short-circuit) — implemented, its cost when disabled not yet measured; HTTP class-level counters (2xx/3xx/4xx/5xx, rejected-before-world, upgrades, streams); `/_usai/status` (runtime status JSON incl. gauges, revisions, services, tasks, resources, http) and `/_usai/metrics` (Prometheus text) as runtime-owned surfaces (`HttpConfig.serve_status`; `usai run --status`, on in `usai dev`); `usai graph` renders workload → resource [lease] / dispatch → task edges from the definition; `--log-format json`. 1 acceptance test + 1 unit test.
- **D13 hardening (first pass).** Service restart policy (`restart: { mode: never|on-failure|always, backoffMs, maxRestarts }`, doubling backoff, stop always wins). Graceful shutdown drains with a bound; a second SIGINT forces exit and reports live worlds/ops. `usai bench` (engineering measurement, prints percentiles, req/s, RSS high-water, and asserts ownership returned to baseline). Connection-loss SQLSTATEs (`08*`, `57P0x`) quarantine the connection instead of returning it. `docs/THREAT-MODEL.md`. 6 tests in `tests/hardening.rs`: memory limit faults cleanly and the runtime continues; failing service restarts per policy then settles; revision replacement under 8 concurrent clients loses no request; tampered/unsupported/missing artifacts are refused with clear errors; budget exhaustion refuses at once (503) and recovers; killed PostgreSQL backend → quarantine → recovery on a replacement connection.
- **D14 developer preview groundwork.** `docs/GUIDE.md` (install → concepts → every workload kind → resources → operate); `create-usai <dir>` scaffolds a runnable project from a template (tested). Not yet an alpha: see Next.
- **D15 control surface.** `usai run --control 127.0.0.1:3900` serves a generic JSON API (`control.rs`): `GET /health`, `GET /status`, `GET /revisions`, `POST /revisions {artifact}` (install from an artifact directory), `POST /revisions/{id}/activate`, `POST /revisions/{id}/drain`, `DELETE /revisions/{id}` (installed/retired only), `POST /stop`. Bearer token from `USAI_CONTROL_TOKEN`; binding off loopback without a token is refused. `Runtime::remove`. 2 tests. Sakala is one client of this protocol, never a dependency.
- **`usai/test` harness** (`GOAL.md` §36): `testApp({ root })` spawns `usai run --port 0 --control 127.0.0.1:0 --announce`, drives HTTP, and invokes tasks / cron ticks / commands deterministically through the control surface's `POST /invoke`. Tested against `examples/hello` with the repository binary.
- `usai test` runs the project's `node --test` files with `USAI_BIN` set to the running binary and the `usai` export condition (workspace checkouts need no `dist/`). OpenAPI describes streams (`x-usai-stream`) and sockets (`x-usai-socket`, 101/426) explicitly. WebSocket idle timeout (`HttpConfig.socket_idle_timeout`, 300 s default, close 1008; tested). ADR-0015 records the measured per-world numbers and states that its revisit trigger has fired.
- Measured bundle composition (release): SDK only 1.0 ms/world, `zod/mini` 1.3 ms/world, `zod` 6.6 ms/world (`tests/profile_bundles.rs`). `zod/mini` lacks Standard JSON Schema, so before-world validation and OpenAPI degrade with it; documented in the guide.
- **PostgreSQL TLS** (rustls, `sslmode` from the URL, roots = Mozilla + `tls.caFile`/`PGSSLROOTCERT`, always verified; refused at activation otherwise). Tested against an embedded TLS server with a private CA.
- **`usai dev` reload acceptance** (`crates/usai-cli/tests/dev_reload.rs`): edit → new revision, no failed request during the swap; a broken edit keeps the previous revision serving. Control surface: rollback = reinstall + activate (tested).
- **P1/P2 substrate economics.** Per-phase ledger (`USAI_PROFILE=1`), workload matrix (`tests/profile_matrix.rs`, `scripts/p1-attribution.sh`), research-VM attribution; slot-reset root fix (vendored Wasmtime patch), callback-free validator preparation in the image (`runtime/prepare.ts`), zero-delay timer yield, lock-free watchdog. Hello on the VM: 1.52 ms p50, 9.8k req/s at c=16, 0 faults.
- **P0 substrate (ADR-0016).** `engine/wasm.rs`: Wasmtime 48 + core built from pinned sources (`guest/build.sh`, `-O3`; `-Oz` reproduces the research core byte for byte) + Wizer image per definition + pooling/COW/pagemap_scan; guest ABI unchanged (bridge routes through the core's `__usai_test_op`); one artifact (IIFE bundle) serves both engines; compilation cache; epoch-based CPU slice. `wasm` is the default engine, `quickjs` selectable. All 71 acceptance tests pass on both. Profiling harness: `tests/profile_bundles.rs` (`--ignored`, release).
- Design review closed 14 of 16 open questions as ADR-0001…0014; ADR-0015 records the engine decision.

## In progress

- nothing

## Measured (2026-09-17, release build, this machine, `usai bench` on `examples/hello`, engineering numbers — not canonical evidence)

```text
c=1    p50 8.2 ms   p99 9.9 ms    121 req/s
c=16   p50 30 ms    p99 45 ms     514 req/s    RSS high-water 133 MB
```

Attribution (`tests/profile_world.rs`, `cargo test --release --test profile_world -- --ignored --nocapture`):

```text
engine floor (fresh runtime + context)        0.88 ms/world
tiny module                                   0.72 ms/world
hello bundle (765 KB, zod evaluated per world) 6.52 ms/world
```

~90% of per-world cost is **application module evaluation per world** — exactly the "definition-level work rebuilt per world" the research removed with a pre-initialized image (Wizer) + copy-on-write memory (C13, EXP-011B/012B). The native QuickJS substrate (ADR-0015) has no snapshot mechanism, so this cost is structural to v0's engine choice, not to the lifecycle model. This is the concrete trigger ADR-0015 named for revisiting the substrate.

## Next

1. **Soak** (≥ 1 h at c=16 on the VM) on the new reset path, watching RSS and the gauge baseline; then propose the Wasmtime region-budget change upstream. Remaining lever: the eval-based invoke/outcome/pending floor (0.24 ms of 1.05) — a core ABI change, after the soak.
2. Acceptance audit done (`docs/ACCEPTANCE-AUDIT.md`); remaining ◐: crash/restart recovery is the orchestrator's, tutorial application, first tag.
3. Per-world CPU accounting (threat model "Open").
5. D14 alpha checklist still open: publish `usai` / `create-usai` to npm and a `usai` binary (today the CLI is `cargo run -p usai-cli`); artifact byte format + signing (ADR-0005 follow-up).

## Known gaps / debt

- The Wasm substrate is the research representation (ADR-0016); its per-world cost is now attributed on the research VM but not yet reduced, and no soak has run. Do not cite EXP-012B numbers for this codebase. The native QuickJS engine stays as reference; do not use it for economics.
- Boundary contracts are validated twice when a JSON Schema exists (host before the world, provider inside it to obtain parsed values). Acceptable for v0; `GOAL.md` §13 asks to collapse this later.
- Auth resolvers run inside the world (after structural validation); `inspect` says so.
- `create-usai` is a placeholder; `examples/hello` is the onboarding path for now.
- Module-level `migrations:`/`seeders:` globs are root-relative, not module-relative (the bundle has no source locations); documented on `defineModule`. PostgreSQL: no transactions across operations yet.
- HTTP/1.1 only; no TLS termination (expected behind a proxy in v0).
