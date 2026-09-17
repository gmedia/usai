# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

**Read [AGENTS.md](AGENTS.md) first.** It is the working agreement for this repository and is shared with every coding agent; this file only adds what is specific to Claude Code. Do not duplicate its content here — link to it.

## What this repository is

The **production** repository for Usai (`gmedia/usai`): a lifecycle-native backend runtime — persistent Rust host, immutable application definition, fresh execution world per unit of work, TypeScript developer surface. Apache-2.0.

The model was proven in the separate research repository `HasanH47/usai` (local checkout `~/Projects/pribadi/experiments/usai/`). This repo inherits its contracts and lessons, **not** its code, structure, or research ceremony (ADR-0001). `docs/RESEARCH-REFERENCE.md` maps every contract to its evidence and tells you which research files to read.

Session start: `docs/STATUS.md` (where we are) → the `GOAL.md` section relevant to the task → `docs/LIFECYCLE-CONTRACTS.md` (what code must uphold).

## Commands

**None exist yet.** The repository currently holds only documentation (`README.md`, `GOAL.md`, `AGENTS.md`, `docs/`). Do not assume or invent `cargo`, `npm`, or `make` targets in answers. Creating them is milestone D0; when they land, replace this section with the real build / lint / test / single-test commands and keep `AGENTS.md` §9 in sync.

## Architecture in one screen

Five lifetimes, always distinct in code (`docs/LIFECYCLE-CONTRACTS.md` C1):

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
