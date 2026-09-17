# ADR-0003: Response API — plain return, explicit helpers, raw `Response` escape hatch

**Status:** accepted (v0)
**Date:** 2026-09-17
**Closes:** Q2

## Context

`GOAL.md` §13 wants returning data to be easy for contract-aware endpoints while keeping explicit status possible and preserving a raw path (§15). The open question was whether to require a response object, use helpers, or allow plain values.

## Decision

Three tiers, in order of how often they should appear:

```text
common   return user;                 runtime encodes per the declared response contract, default status
special  return http.created(user);   small helpers for explicit status/headers (exact names may change)
raw      return new Response(...);    escape hatch; contract enforcement and docs become partial for that response
```

The runtime selects the declared default status, applies the response contract, and encodes once (no duplicate validate-then-serialize passes where a schema-aware encoder can combine them).

## Consequences

- Easier: the typical handler is one `return`.
- Harder: three shapes must be distinguishable at compile time and at runtime; raw responses must be reported as opaque in OpenAPI (C8).
- Forbidden: forcing every handler to construct a `Response`; silently accepting a raw `Response` where a contract was declared without marking the endpoint's docs partial.

## Alternatives considered

- **Always return a `Response` object.** Rejected: maximal ceremony for the 90% case.
- **Only plain values, no raw path.** Rejected by `GOAL.md` §15 — webhooks, streaming, and proxies need bytes-level control.
