# Status

> Where the project is right now. Update this in the same change that moves it.

**Last updated:** 2026-09-17

## Current milestone

**D2 — HTTP Contract Workload** (next). D0 and D1 are done.

## Done

- **D0 foundation.** Rust workspace (`crates/usai-runtime`, `crates/usai-cli`), pnpm workspace (`packages/usai`, `packages/create-usai`, `examples/hello`), `make check` (fmt, clippy `-D warnings`, cargo test, tsc, node --test), GitHub Actions CI, pinned Rust 1.98.1 / Node 24 / pnpm 12.
- **D1 lifecycle core** in `usai-runtime`: immutable `ApplicationDefinition` + manifest; engine boundary with the QuickJS-ng substrate (ADR-0015) and the guest bridge ABI (`docs/GUEST-ABI.md`); `WorldDriver` with one identity-first completion gate; bounded live-ownership `Ledger` + gauges; host operations (timer, resource) with independent owners; `ResourceManager`/`ResourceIdentity`/`cache.local`; hierarchical `Budget` admission; `Runtime` with revisions `installed → active → draining → retired`. 17 lifecycle acceptance tests (`crates/usai-runtime/tests/lifecycle.rs`) cover isolation, resource persistence, baseline return, cancel, deadline, detached work, runaway CPU, budget refusal, revision replacement, failed activation, shutdown.
- Design review closed 14 of 16 open questions as ADR-0001…0014; ADR-0015 records the engine decision.

## In progress

- nothing

## Next (D2 acceptance: contract-aware endpoint end-to-end; invalid boundary input fails before world creation; fresh world per request; correct response ownership; concurrent requests do not leak state)

1. `usai` SDK: `defineApp`, `defineModule`, `http.get/post/…`, `http.raw`, `errors`, `env`, resource declarations; `__usai_sdk.invoke` on top of the bridge; manifest extraction (`describe`).
2. Runtime HTTP host: hyper server, `matchit` router from the definition, decode → JSON Schema validation (`jsonschema`) → auth boundary → admission → world → response contract → error mapping; raw escape hatch; client-disconnect cancellation.
3. Build pipeline: esbuild bundle + controlled evaluation to emit `manifest.json` + `app.js` (ADR-0009).
4. `examples/hello` becomes runnable.

## Known gaps / debt

- The QuickJS substrate is native, not the Wasmtime pooling+COW representation the research measured (ADR-0015). Do not cite EXP-012B economics for it.
- Auth resolvers run inside the world (after structural validation) in v0; ADR-0004 allows this and `inspect` must say so.
- `create-usai` is a placeholder until `usai dev` exists (D3).
- No PostgreSQL provider yet (D6).
