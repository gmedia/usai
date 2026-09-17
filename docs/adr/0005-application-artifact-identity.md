# ADR-0005: The Usai artifact is a logical, versioned artifact; engine-compiled output is host cache

**Status:** accepted (v0, format details deferred)
**Date:** 2026-09-17
**Closes:** Q4 (identity and layering; exact byte format remains a D3 detail)

## Context

`GOAL.md` §30–31 separate application definition from deployment instance and ask for deterministic builds. The research repo's artifacts were engine-specific Wasm images with pinned digests. Binding the public artifact identity to the engine representation would make every engine upgrade a breaking artifact change and would leak internals (`GOAL.md` §49).

## Decision

A `.usai` application artifact is a **logical** unit whose identity belongs to Usai:

```text
manifest                       artifact format version, application name, revision identity
ApplicationDefinition metadata workloads, contracts, resources, config requirements, migrations/seeders
compiled/bundled payload       the application code in the build pipeline's portable form
content identity               hash over the above
```

Engine-specific compiled representations (e.g. a Wasmtime-compiled module, a pre-initialized image) are **host cache**, keyed by artifact identity + engine identity, never part of the artifact's public identity.

## Consequences

- Easier: engine changes do not invalidate artifacts; `inspect`, Sakala, and docs read the artifact without an engine; content addressing and rollback key on Usai identity.
- Harder: first activation on a host may pay a compile step; the cache needs its own lifetime and invalidation rules (who owns it, what bounds it — `AGENTS.md` §4).
- Forbidden: artifact identity that changes when only the engine version changes; exposing engine cache paths as configuration.
- Revisit: exact byte format, signing, and compatibility policy at D3 (`usai build`) and D14; record as a follow-up ADR.

## Alternatives considered

- **Artifact = engine image (research style).** Rejected: couples releases to engine internals and violates `GOAL.md` §49.
- **Source-only artifact, compile at runtime.** Rejected: nondeterministic activation cost and no build-time validation.
