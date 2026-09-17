# AGENTS.md — working agreement for Usai (production repository)

This file is the working agreement for any coding agent (Claude Code, Codex, Gemini CLI, Cursor, humans) operating in `gmedia/usai`. Tool-specific files (`CLAUDE.md`, etc.) point here and add only what is specific to that tool.

Read order for a new session:

1. this file;
2. `docs/STATUS.md` — where the project currently is;
3. `GOAL.md` — product/engineering north star (long; skim the section you need);
4. `docs/LIFECYCLE-CONTRACTS.md` — the invariants code is checked against;
5. `docs/GLOSSARY.md` — shared vocabulary, including research-era terms you will meet in the evidence.

---

## 1. What this repository is, and is not

`gmedia/usai` is the **production** implementation of Usai: a lifecycle-native backend runtime. Persistent Rust host, immutable application definition, fresh execution world per unit of work, TypeScript developer surface.

It is **not** the research repository. The research repository is `HasanH47/usai` (local checkout: `~/Projects/pribadi/experiments/usai/`). That repo proved the model through sealed, one-shot experiments and remains the source of evidence. This repo inherits its **contracts and lessons, not its code, structure, or ceremony**. See `docs/RESEARCH-REFERENCE.md` for the map.

Hard rules about the boundary:

```text
never add a build/runtime dependency on the research repo
never vendor experiment crates, benchmark controllers, judges, freeze/seal machinery
never edit sealed records in the research repo (*-RESULT.md, *-FREEZE.json, artifacts/)
never copy research ceremony (preregistration, freezes, one-shot attempts) into this repo
```

---

## 2. Work mode: ordinary engineering

This repository operates in normal software-engineering mode:

```text
design -> implement -> test -> benchmark -> profile -> improve -> regression test -> release
```

No preregistration, freeze manifests, one-shot claims, evidence archives, or external adjudication. Benchmarks here are engineering tools. Iterate freely.

Formal research is reserved for questions that (a) could materially change the runtime architecture, (b) cannot be answered by tests/profiling/comparison, (c) are vulnerable to benchmark retuning, and (d) are costly enough to justify ceremony. If you hit one, say so and stop; do not run a canonical study from this repo. Examples that do **not** need research: a slow function, an allocation hotspot, a routing implementation choice, CLI formatting, a library with a clear practical winner.

---

## 3. Autonomy: decide locally, surface strategically

Make ordinary local engineering decisions without asking. Prefer the smaller coherent design, fewer abstractions, explicit ownership, explicit lifetime boundaries, testability, deterministic application models, minimal public API.

**Stop and surface the decision** (do not silently decide) when a change would:

- contradict a contract in `docs/LIFECYCLE-CONTRACTS.md`;
- materially alter the runtime thesis;
- introduce a new engine/runtime representation (different JS engine, non-Wasm substrate, microVM, etc.);
- introduce or claim a security boundary;
- create irreversible public API compatibility;
- create implicit detached work;
- introduce a persistence model whose concurrency/durability semantics are unclear;
- rest on performance assumptions the evidence does not support;
- decide one of the questions in `docs/OPEN-QUESTIONS.md`.

Do not stop merely because the implementation is difficult.

Decisions in the strategic category are recorded as ADRs in `docs/adr/`. Open questions stay in `docs/OPEN-QUESTIONS.md` until an ADR closes them.

---

## 4. Lifetime discipline in code

Keep five lifetimes distinct in the code. Never collapse two because an implementation happens to make them adjacent:

```text
persistent runtime state
immutable application definition
execution world (work-scoped mutable state)
resource lease
external operation
```

For every new cache, pool, registry, worker, background task, or process-lifetime structure, be able to answer in the PR/commit:

```text
who owns it?
what is its natural lifetime?
what bounds its growth (in time and in space)?
what happens on cancellation / crash?
what proves it is safe to reuse?
```

Any process-lifetime structure that grows per request — in time or in space — is a defect until attributed.

Profile before changing architecture for performance. The research lineage (EXP-012A-R1 → EXP-012B) exists because host work the semantics never required — linear ledger scans, per-request definition rebuilds, eager formatting for disabled events, full kept-memory resets — was mistaken for the cost of the lifecycle model.

---

## 5. Milestones and scope

Build in the order of `GOAL.md` §53 (D0 foundation → D1 lifecycle core → D2 HTTP → D3 dev loop → D4 tasks → D5 cron/commands → D6 PostgreSQL → D7 project model → D8 OpenAPI → D9 service → D10 queue → D11 socket/stream → D12 observability → D13 hardening → D14 alpha → D15 Sakala). Each milestone has acceptance criteria there. `docs/STATUS.md` says which one is current.

D1 (lifecycle core, testable without HTTP) is more important than routing. Do not start D2 before D1's acceptance holds.

Non-goals before alpha (`GOAL.md` §54): new language, ORM, Node compatibility layer, native addons, plugin marketplace, package registry, distributed scheduler, multi-region, microVM sandbox, custom JS engine, arbitrary durable-object model, broad capability catalog, MVC/DI framework, Sakala coupling. Do not build them because they would be easy.

Repository layout: start with `GOAL.md` §48 (`crates/usai-runtime`, `crates/usai-cli`, `packages/usai`, `packages/create-usai`, `examples/`, `docs/`, `tests/`). Split a crate only for a concrete dependency, compile-boundary, API-stability, reuse, or build-performance reason — not per semantic noun.

---

## 6. Claims discipline

Research evidence must not be rewritten as marketing. The project may say:

```text
serious runtime development is justified
request-native semantics survived the registered HTTP+PG concurrency study
R3 economics stayed inside the inherited ratio envelope through c = 64
```

It may **not** say:

```text
production-ready
internet-scale proven
generally faster than Node / Bun / Deno / PHP / Rust
secure multi-tenant sandbox proven
all backend workloads supported
```

Production readiness requires production-shaped evidence: soak, overload, crash/restart, rolling upgrade, multi-core, many-app density, threat model. None of that exists yet.

---

## 7. How a turn should go

1. **Verify before asserting.** Never state that a file, function, command, threshold, or result exists without looking. Never say a command passed unless it ran.
2. **Audit the idea before implementing it.** What problem does it solve, what lifetime does the state really need, is there a simpler design?
3. **Separate fact, interpretation, and hypothesis** — especially for performance and lifecycle claims.
4. **State verification limits.** "Inspected source but did not run it" is useful information.
5. **Finish the milestone slice you started.** Tests, docs, and `docs/STATUS.md` are part of the change, not follow-ups.
6. **When you find a defect in your own earlier work, say so and fix it.** Do not rewrite history to hide it.

---

## 8. Conventions

- Code, identifiers, commit messages, and repository docs are in **English**. Explanations to the user are in **Indonesian**.
- Comments explain *why* — a constraint, invariant, or surprising lifetime/ownership choice — not what the line already says. Match the density and idiom of the surrounding module.
- Failure-path tests matter most around cancellation, resource ownership, reuse, shutdown, and delivery after world death. A happy-path-only test for ownership code is incomplete.
- Commit messages: concise and factual; state the behavior/invariant changed and the tests run.
- Do not expose engine internals (Wasmtime options, pooling slots, COW, pagemap, memory reservation) as application configuration. They are runtime internals or operator/debug surfaces.
- Do not freeze public API before the first developer preview. Prefer semantic clarity; allow breaking changes; use explicit experimental namespaces where needed.

---

## 9. Repository map

```text
README.md                     public entry point
GOAL.md                       product & engineering north star (binding invariants, open syntax)
AGENTS.md                     this file
CLAUDE.md                     Claude Code pointer + tool-specific notes
LICENSE                       Apache-2.0
docs/README.md                documentation index
docs/STATUS.md                current milestone, done / in progress / next
docs/LIFECYCLE-CONTRACTS.md   binding runtime invariants, phrased as testable rules
docs/GLOSSARY.md              vocabulary, including research-era terms
docs/RESEARCH-REFERENCE.md    map from production concepts to research evidence
docs/OPEN-QUESTIONS.md        design questions that must not be decided by accident
docs/adr/                     architecture decision records
```

Code directories appear as milestones land; update this map when they do.
