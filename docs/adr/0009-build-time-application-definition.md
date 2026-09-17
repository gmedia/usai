# ADR-0009: The ApplicationDefinition is constructed at build time; the runtime loads it, it does not discover it

**Status:** accepted (v0)
**Date:** 2026-09-17
**Closes:** Q11

## Context

Many frameworks discover routes by executing application bootstrap code at startup and observing side effects. That makes topology depend on arbitrary execution, breaks `GOAL.md` §6 ("avoid hidden topology"), §31 (determinism), and makes `inspect`/docs/Sakala analysis require running the app.

## Decision

```text
TypeScript source
   ↓
Usai build phase: controlled evaluation/transform of declarations only
   ↓
validated, static ApplicationDefinition  +  compiled application payload
   ↓
runtime loads the definition; serves work
```

Invariant: **the runtime loads a built definition; it never discovers topology by executing arbitrary business bootstrap logic.** Whether the build phase uses AST transforms, controlled evaluation of `defineApp`/`defineModule`, or a hybrid is an internal detail and may change.

Declarations (`defineApp`, `defineModule`, `http.get`, `task`, `cron`, resources) must therefore be statically analyzable or evaluable without side effects (C9 determinism applies to declarations too).

## Consequences

- Easier: `inspect`, `graph`, OpenAPI, and validation work without running handlers; failed definitions fail at build, not at first request; same definition in tests and production.
- Harder: declaration code has restrictions (no I/O, no clock, no conditional topology on runtime env); the build phase needs clear diagnostics when a declaration is not analyzable.
- Forbidden: runtime filesystem scanning for routes; topology that depends on `process.env` at runtime; registering workloads from inside handlers.

## Alternatives considered

- **Runtime bootstrap discovery.** Rejected: nondeterministic and un-inspectable.
- **Pure AST-only, no evaluation.** Not rejected — it is one permitted implementation; the ADR fixes the invariant, not the technique.
