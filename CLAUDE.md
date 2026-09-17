# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

**Read [AGENTS.md](AGENTS.md) first.** It is the working agreement for this repository and is shared with every coding agent; this file only adds what is specific to Claude Code. Do not duplicate its content here — link to it.

## What this repository is

The **production** repository for Usai (`gmedia/usai`): a lifecycle-native backend runtime — persistent Rust host, immutable application definition, fresh execution world per unit of work, TypeScript developer surface. Apache-2.0.

The model was proven in the separate research repository `HasanH47/usai` (local checkout `~/Projects/pribadi/experiments/usai/`). This repo inherits its contracts and lessons, **not** its code, structure, or research ceremony (ADR-0001). `docs/RESEARCH-REFERENCE.md` maps every contract to its evidence and tells you which research files to read.

Session start: `docs/STATUS.md` (where we are) → the `GOAL.md` section relevant to the task → `docs/LIFECYCLE-CONTRACTS.md` (what code must uphold).

## Commands

```bash
make setup        # pnpm install --frozen-lockfile + cargo fetch
make check        # fmt-check + lint + test (what CI runs)
make lint         # cargo clippy --workspace --all-targets -- -D warnings; pnpm -r run typecheck
make test         # cargo test --workspace; pnpm -r run test
make build        # release build + tsc for packages
make fmt          # cargo fmt --all
```

Developer loop on an example:

```bash
cargo run -p usai-cli -- dev --root examples/hello        # build, serve on :3000, rebuild on change
cargo run -p usai-cli -- inspect --root examples/hello    # what the runtime understood (add --json for the manifest)
cargo run -p usai-cli -- config --root examples/hello     # effective project config + sources
```

Single tests:

```bash
cargo test -p usai-runtime --test lifecycle detached_timer          # one integration test by name
cargo test -p usai-runtime --test http auth_boundary                # HTTP tests need node + `pnpm install` (they skip otherwise)
cargo test -p usai-runtime --lib ownership::                        # one module's unit tests
node --test packages/usai/src/index.test.ts                         # one TS test file (Node 24 strips types natively)
```

Lifecycle tests use `#[tokio::test(flavor = "multi_thread")]` on purpose: guest execution blocks its worker thread, and a single-thread executor cannot host the completion owners alongside it.

Toolchain pins: Rust 1.98.1 (`rust-toolchain.toml`), Node ≥ 24, pnpm 12.3.4 (`package.json#packageManager`). `rquickjs` compiles QuickJS-ng from source with `cc`; the first build takes ~20 s.

## Architecture in one screen

Five lifetimes, always distinct in code (`docs/LIFECYCLE-CONTRACTS.md` C1). In `crates/usai-runtime/src`: `runtime.rs` (persistent, revisions) → `definition.rs` (immutable) → `world.rs` (`WorldDriver`, one routing gate) → `host_ops.rs` (operation owners) / `resource/` (leases, terminal proof); `ownership.rs` is the bounded ledger; `engine/` is the substrate boundary (only `engine/quickjs.rs` may import `rquickjs`); `engine/guest-bridge.js` + `docs/GUEST-ABI.md` is the host↔guest contract; `http/` is the request pipeline (route → decode → validate → admit → world → encode); `build.rs` is the build phase (esbuild via the SDK's `build/bundle.mjs`, then manifest extraction in a capability-less world). The TypeScript side is `packages/usai/src`: declarations → `manifest.ts` (`describe`) and `runtime/sdk.ts` (`invoke`) must agree on workload ordinals via `flatten()`.

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
