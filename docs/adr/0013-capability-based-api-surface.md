# ADR-0013: Capability-based, minimal Web-like API surface; no Node compatibility target

**Status:** accepted (v0 philosophy)
**Date:** 2026-09-17
**Closes:** Q15

## Context

`GOAL.md` §4/§54 exclude Node compatibility as a goal; `RESEARCH-STATE.md` §11.7 calls compatibility a budget. An API is only safe to expose when its lifetime semantics under a disposable world are clear.

## Decision

The guest surface is: **Web-standard primitives where their lifetime is clear** + **Usai lifecycle-aware primitives**. Availability is itself part of the capability model.

| API | v0 stance | Lifetime note |
|---|---|---|
| `URL`, `TextEncoder/Decoder`, JSON, structured clone | yes | pure |
| `crypto` (WebCrypto subset) | yes | pure / host-provided |
| `fetch` | likely yes | an **external operation** owned by the world; cancelled on world end |
| timers | yes | owned by the world; a live timer at finite-world end is a C3 violation |
| `process`, `require`, Node globals | no | — |
| `child_process` | no by default | — |
| filesystem | capability/permission question; not in v0 default surface | — |
| raw network sockets | explicit capability only | — |

Adding an API requires answering: who owns it, what happens at world end, what happens on cancel.

## Consequences

- Easier: small, auditable surface; every API has a lifetime story; no chasing Node.
- Harder: some npm packages will not run; the compatibility budget must be documented and revisited by demand.
- Forbidden: polyfilling Node globals to make arbitrary packages work; exposing an API without a lifetime rule.

## Alternatives considered

- **Node compatibility layer.** Rejected by `GOAL.md` and by the lifecycle model (Node globals assume process lifetime).
