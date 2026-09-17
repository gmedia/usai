# ADR-0002: Schema strategy — Standard Schema for validation, JSON Schema as a separate metadata capability

**Status:** accepted (v0)
**Date:** 2026-09-17
**Closes:** Q1

## Context

Contract-aware workloads (HTTP params/query/headers/body/response, task input, queue message) need runtime validation, TypeScript inference, and — for OpenAPI and generated clients — enough metadata to describe the shape. `GOAL.md` §43 forbids inventing a schema language merely to own one, and asks for no forced dependency on a single validator library.

## Decision

```text
validation protocol       Standard Schema-compatible (any library implementing the spec)
documentation metadata    a separate, optional JSON Schema capability the provider may expose
built-in Usai schema      none in v0
```

Zod, Valibot, ArkType, or any Standard Schema implementer is accepted wherever a contract is declared. Validation always works. Documentation quality depends on whether the provider exposes JSON Schema metadata; if it does not, the endpoint still runs and the generated docs for it are marked degraded/opaque rather than fabricated.

## Consequences

- Easier: no schema library lock-in; developers keep their existing schemas; the runtime owns only a thin adapter.
- Harder: OpenAPI fidelity varies by provider; the runtime must represent "validated but undocumented" honestly (`LIFECYCLE-CONTRACTS.md` C8).
- Forbidden: a Usai-specific schema DSL; treating "can validate" as "can document".
- Revisit: if a concrete need arises that no existing ecosystem can meet cleanly (`GOAL.md` §43), write a new ADR before building a schema library.

## Alternatives considered

- **Zod as the only supported library.** Rejected: forces a dependency and couples runtime versioning to one library's release cadence.
- **JSON Schema as the primary contract.** Rejected: poor TypeScript inference and ergonomics for the common case.
- **Built-in schema library from day one.** Rejected by `GOAL.md` §43 unless it solves a problem existing ecosystems cannot.
