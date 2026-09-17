# Status

> Where the project is right now. Update this in the same change that moves it.

**Last updated:** 2026-09-17

## Current milestone

**D0 — Production Repository Foundation** (`GOAL.md` §53). Not started beyond documentation.

## Done

- Repository created (`gmedia/usai`, Apache-2.0).
- `README.md`, `GOAL.md` (product direction), agent working agreement (`AGENTS.md`, `CLAUDE.md`), and `docs/` foundation (contracts, glossary, research reference, open questions).
- Design review closed 14 of 16 open questions as ADR-0001…0014 (v0 stances). Only Q8 (multi-core) and Q10 (p50 gap) remain open as empirical questions.

## In progress

- nothing

## Next (D0 acceptance: clean clone builds; tests pass with documented commands; no dependency on the research repo; repository is clearly production-oriented)

1. Rust workspace (`crates/usai-runtime`, `crates/usai-cli`) with pinned toolchain, `fmt` / `clippy -D warnings` / `test`.
2. TypeScript workspace (`packages/usai`, `packages/create-usai`) with lint/typecheck/test.
3. One top-level command set (e.g. `make check` / `make test` or a task runner) and CI running it.
4. `examples/hello` placeholder so the layout is real.
5. Record the commands in `CLAUDE.md` and `AGENTS.md` §9 once they exist.

## Known gaps / debt

- No commands exist yet; agents must not assume any.
- Toolchain pins (Rust, Node, Wasmtime, QuickJS build) are not yet chosen for this repo. The research baseline is listed in `docs/RESEARCH-REFERENCE.md`; pinning is a D0 decision, not an inheritance.
