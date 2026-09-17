# ADR-0006: Revision lifecycle `installed → active → draining → retired`; dev reload is rapid revision replacement

**Status:** accepted (v0)
**Date:** 2026-09-17
**Closes:** Q5, Q7

## Context

An ApplicationDefinition is immutable (C1). Both `usai dev` reload (`GOAL.md` §38) and production deployment (§53 D15) need a way to move from one definition to the next without mutating the active one or corrupting in-flight ownership.

## Decision

One mechanism for both:

```text
Revision A active
   │  build B
   ▼
Revision B installed        validated, not yet serving
   │  activation succeeds
   ▼
Revision B active
Revision A draining         accepts no new work; in-flight worlds settle
   │  all owned work settled
   ▼
Revision A retired
```

- Failed installation or activation of B leaves A active and untouched.
- No mutation of an active definition, ever. No per-module hot surgery in v0.
- `usai dev` is **rapid revision replacement** with the same states; it may feel like hot reload but is not one.
- Persistent resources are reused across revisions only under ADR-0011's identity rules.

## Consequences

- Easier: dev and production share one tested path; reload can never leave a world half-old/half-new; observability keys on revision identity.
- Harder: dev reload latency is bounded by build + install + activate, so the build pipeline must be fast; draining needs a bound and a diagnostic for stuck work.
- Forbidden: in-place edits to an active definition; module-level patching of a running revision.
- Revisit: per-module rebuild as a build-time optimization (not a runtime one) if dev latency demands it.

## Alternatives considered

- **Process restart on every change.** Rejected by `GOAL.md` §38 ("designed, not hacked around process restart forever").
- **Per-module hot swap.** Rejected for v0: breaks immutability and makes ownership during swap undefined.
