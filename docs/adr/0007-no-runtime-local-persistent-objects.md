# ADR-0007: No arbitrary runtime-local persistent object primitive in v0

**Status:** accepted (v0)
**Date:** 2026-09-17
**Closes:** Q6

## Context

`GOAL.md` §25 warns that `persistent(() => new Map())` creates unresolved concurrency, durability, replication, restart, and multi-node semantics. The open question was whether a safe explicit runtime-local primitive is still needed beyond resources.

## Decision

There is no `runtime.persist(...)` / `persistent(...)` primitive. State that outlives a world is expressed only through explicit contracts:

```text
runtime-local, non-durable, shared     cache.local(...)        resource
durable                                postgres(...) etc.      resource
connection-local                       ctx.state               connection-bound workload
service-local                          locals in the service   persistent workload
```

If a use case cannot be expressed well with these, design a **new resource type with a stated contract** (sharing, durability, restart behavior), via ADR — not a generic mutable object.

## Consequences

- Easier: every persistent value has a declared lifetime and owner; no hidden process-global state.
- Harder: some conveniences (a quick in-memory counter across requests) require declaring a cache resource.
- Forbidden: exposing engine-level persistent globals; promoting connection or world state to runtime scope implicitly.

## Alternatives considered

- **Durable-object style primitive.** Rejected for v0 (`GOAL.md` §54).
- **Allow persistent objects but single-threaded.** Rejected: locks in a concurrency model before Q8 (multi-core) is answered.
