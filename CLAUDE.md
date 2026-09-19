# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

**Read [AGENTS.md](AGENTS.md) first.** It is the working agreement for this repository and is shared with every coding agent; this file only adds what is specific to Claude Code. Do not duplicate its content here — link to it.

## What this repository is

The **production** repository for Usai (`gmedia/usai`): a lifecycle-native backend runtime — persistent Rust host, immutable application definition, fresh execution world per unit of work, TypeScript developer surface. Apache-2.0.

The model was proven in a separate, **private** research repository (maintainers only; a local checkout may exist — ask the user for its path; never name or link it in public-facing text). This repo inherits its contracts and lessons, **not** its code, structure, or research ceremony (ADR-0001). `docs/RESEARCH-REFERENCE.md` maps every contract to its evidence and tells you which research files to read.

Session start: `docs/STATUS.md` (where we are) → the `GOAL.md` section relevant to the task → `docs/LIFECYCLE-CONTRACTS.md` (what code must uphold).

## Commands

```bash
make setup        # pnpm install --frozen-lockfile + cargo fetch
make check        # fmt-check + lint + docs-check + test (what CI runs)
make lint         # cargo clippy --workspace --all-targets -- -D warnings; pnpm -r run typecheck
make docs         # regenerate docs/sdk (TypeDoc → markdown) from the SDK's doc comments; docs-check fails CI when stale
make test         # cargo test --workspace; pnpm -r run test
make build        # release build + tsc for packages
make fmt          # cargo fmt --all
```

Developer loop on an example:

```bash
cargo run -p usai-cli -- dev --root examples/hello        # build, serve on :3000, rebuild on change
cargo run -p usai-cli -- inspect --root examples/hello    # what the runtime understood (add --json for the manifest)
cargo run -p usai-cli -- config --root examples/hello     # effective project config + sources
cargo run -p usai-cli -- --root <project> app <command> [args]   # run a declared command in a fresh world
cargo run -p usai-cli -- --root <project> cron run <name>        # one cron invocation, no wall clock
cargo run -p usai-cli -- --root <project> task run <name> --input '{...}'
cargo run -p usai-cli -- --root <project> db migrate | db status | db seed [name]
cargo run -p usai-cli -- --root <project> generate openapi [--out openapi.json]
cargo run -p usai-cli -- --root <project> graph                    # workload → resource / dispatch graph
cargo run -p usai-cli -- --root <project> test [files]             # node --test with usai/test pointed at this binary
cargo run -p usai-cli -- --log-format json run --status ...       # JSON logs; /_usai/status + /_usai/metrics
cargo run -p usai-cli -- --root <project> run --artifact .usai/build --control 127.0.0.1:3900   # orchestrator surface (USAI_CONTROL_TOKEN)
cargo run --release -p usai-cli -- --root examples/hello bench --path /hello/x -c 16 -d 10   # engineering load test (release build!)
cargo test --release -p usai-runtime --test profile_bundles -- --ignored --nocapture # per-world / per-request cost, both engines (USAI_ENGINE=wasm|quickjs)
USAI_PROFILE=1 cargo test --release -p usai-runtime --test profile_matrix -- --ignored --nocapture  # workload matrix with the per-phase ledger (USAI_MATRIX_ROWS, USAI_WASM_CORE=<core.wasm>)
scripts/p1-attribution.sh [research-Oz-core.wasm]                                     # ledger for -O3 / -Oz / native + perf-stat counters per request (Linux VM)
USAI_ENGINE=quickjs cargo test -p usai-runtime --test lifecycle                        # run a suite on the reference engine
crates/usai-runtime/guest/build.sh                                                     # rebuild the core (-O3); OPT=-Oz reproduces the research core
docker build -f docker/runtime.Dockerfile -t sakaladev/usai:local . && docker build -f docker/dev.Dockerfile --build-arg USAI_RUNTIME_IMAGE=sakaladev/usai:local -t sakaladev/usai:local-dev .
scripts/container-smoke.sh sakaladev/usai:local sakaladev/usai:local-dev            # scaffold → app image → read-only non-root → 200 → SIGTERM → compose path (what CI runs)
```

Single tests:

```bash
cargo test -p usai-runtime --test lifecycle detached_timer          # one integration test by name
cargo test -p usai-runtime --test http auth_boundary                # HTTP/workload tests need node + `pnpm install` (they skip otherwise)
cargo test -p usai-runtime --test workloads cron_scheduler
cargo test -p usai-runtime --test postgres                          # needs PostgreSQL: USAI_TEST_DATABASE_URL, else downloads a portable PG 18 once (~12 MB)
cargo test -p usai-runtime --lib ownership::                        # one module's unit tests
node --test packages/usai/src/index.test.ts                         # one TS test file (Node 24 strips types natively)
node --test packages/usai/src/test.test.ts                          # usai/test harness; needs target/debug/usai (skips otherwise)
```

Lifecycle tests use `#[tokio::test(flavor = "multi_thread")]` on purpose: guest execution blocks its worker thread, and a single-thread executor cannot host the completion owners alongside it.

Toolchain pins: Rust 1.98.1 (`rust-toolchain.toml`), Node ≥ 24, pnpm 12.3.4 (`package.json#packageManager`). `rquickjs` compiles QuickJS-ng from source with `cc`; the first build takes ~20 s.

## Architecture in one screen

Five lifetimes, always distinct in code (`docs/LIFECYCLE-CONTRACTS.md` C1). In `crates/usai-runtime/src`: `runtime.rs` (persistent, revisions) → `definition.rs` (immutable) → `world.rs` (`WorldDriver`, one routing gate) → `host_ops.rs` (operation owners) / `resource/` (leases, terminal proof; `postgres.rs` is the C5 reference implementation); `ownership.rs` is the bounded ledger; `engine/` is the substrate boundary (`engine/wasm.rs` = Wasmtime + Wizer image + pooling/COW, the default, ADR-0016; `engine/quickjs.rs` = native reference, ADR-0015; only those files may import `wasmtime`/`rquickjs`); `guest/build.sh` rebuilds the core from pinned sources; `engine/guest-bridge.js` + `docs/GUEST-ABI.md` is the host↔guest contract; `http/` is the request pipeline (route → decode → validate → admit → world → encode; `stream.rs` and `socket.rs` are the connection-bound attachments); `workloads/` is tasks (invoke/dispatch ops + `TaskQueue`), cron (per-revision schedulers), services (supervisor + graceful stop), queue (PostgreSQL-backed consumers); `control.rs` is the orchestrator surface (install/activate/drain/remove/stop); `build.rs` is the build phase (esbuild via the SDK's `build/bundle.mjs`, then manifest extraction in a capability-less world). The TypeScript side is `packages/usai/src`: declarations → `manifest.ts` (`describe`) and `runtime/sdk.ts` (`invoke`) must agree on workload ordinals via `flatten()`.

```text
persistent runtime state  →  immutable ApplicationDefinition  →  execution world
                                                                  ├─ resource lease
                                                                  └─ external operation
```

Workload kind gives the default lifetime — finite (HTTP, task, cron, queue message, command, migration, seeder), connection-bound (WebSocket, stream), persistent (service). The rules Claude is most likely to get wrong under time pressure:

- **No implicit detached work** (C3): `invoke` = owned/awaited, `dispatch` = ownership transferred; a finite world ending with live async work is a runtime error with a teaching diagnostic.
- **Death is not cleanup** (C4/C5): world death proves nothing about a query or connection; PostgreSQL reuse only after terminal proof, otherwise quarantine → remove → replace.
- **Fail before the world exists** (C6): routing, decoding, schema, and auth rejections never create a world.
- **One source of truth** (C8): inspect/graph/OpenAPI/tests read the same ApplicationDefinition the runtime executes.
- **Do not re-pay research costs** (C13): no per-request scans of process-lifetime structures, no per-world definition rebuilds, no allocating/formatting for disabled observability.

Milestone order is `GOAL.md` §53 (D0 → D15); D1 lifecycle core before D2 HTTP. Non-goals before alpha are `GOAL.md` §54.

## Claude-specific notes

- Explain to the user in **Indonesian**; write code, identifiers, commit messages, and repo docs in **English**.
- Stop-and-surface decisions (`AGENTS.md` §3) and the rows in `docs/OPEN-QUESTIONS.md` are the cases to use `AskUserQuestion` or state an explicit assumption — not routine engineering choices.
- When a change moves the milestone, edit `docs/STATUS.md` in the same change. When it closes an open question, add an ADR and update `docs/OPEN-QUESTIONS.md`.
- Never edit files under the research repository's `docs/experiments/` or `artifacts/`, even if asked to "fix" something there; add a new document instead and say why.
- Research-era vocabulary (R1/R2/R3, incarnation, Phase S, exp011d) is for reading evidence; translate it via `docs/GLOSSARY.md` before it reaches production code or docs.
