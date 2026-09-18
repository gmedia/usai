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

**Addendum (2026-09-18).** `usai build` writes the engine's serialized compiled form under `cache/` next to the artifact (`cache/image.cwasm` + `cache/image.json` with the engine name, the engine's fingerprint — Wasmtime build, guest core digest, host target — and the digests of the code it was built from and of the file itself). Installing the artifact loads it (~15 ms) instead of compiling (seconds on every core); any mismatch of code digest, file digest, engine or fingerprint makes it ignored with a log line, and the engine compiles from `app.js`. It is host cache in the sense of this ADR: the identity of the artifact is unchanged by its presence, and a deployment may delete it. It is native code, so the artifact directory's trust boundary applies to it (`docs/THREAT-MODEL.md`). Load order is fixed: the logical artifact is verified first (`app.js` must hash to the manifest's `codeSha256`), then `cache/image.json` must name that code digest and the file's own digest must match, then the engine name and fingerprint must match the running engine — only then is the module deserialized. The digests protect against mismatch and corruption, not against a party who can write the directory; artifacts fetched from elsewhere (a registry, an orchestrator) need artifact-level signing before their cache may be trusted, which is this ADR's open follow-up.

## Consequences

- Easier: engine changes do not invalidate artifacts; `inspect`, Sakala, and docs read the artifact without an engine; content addressing and rollback key on Usai identity.
- Harder: first activation on a host may pay a compile step; the cache needs its own lifetime and invalidation rules (who owns it, what bounds it — `AGENTS.md` §4).
- Forbidden: artifact identity that changes when only the engine version changes; exposing engine cache paths as configuration.
- Revisit: exact byte format, signing, and compatibility policy at D3 (`usai build`) and D14; record as a follow-up ADR.

## Alternatives considered

- **Artifact = engine image (research style).** Rejected: couples releases to engine internals and violates `GOAL.md` §49.
- **Source-only artifact, compile at runtime.** Rejected: nondeterministic activation cost and no build-time validation.
