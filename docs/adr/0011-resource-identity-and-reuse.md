# ADR-0011: Resource identity = kind + logical name + configuration fingerprint + compatibility version; reuse across revisions only on identity match

**Status:** accepted (v0)
**Date:** 2026-09-17
**Closes:** Q13

## Context

Resource managers (e.g. a PostgreSQL pool) live at runtime lifetime and may outlive many revisions (ADR-0006). Sharing them across revisions is a major efficiency win, but "same name" is not "same resource": revision B may point `postgres/main` at a different database.

## Decision

```text
ResourceIdentity = kind + logical name + normalized configuration fingerprint + compatibility version
```

- Two revisions share a resource manager **only** when identities are equal.
- A differing fingerprint creates a new manager; the old one drains with its revision.
- Resource contracts declare a compatibility version so a manager implementation change can force a fresh instance even with identical config.
- Secrets are fingerprinted, never stored, in the identity.

## Consequences

- Easier: safe reuse across dev reloads and rolling deploys; no cross-database connection leaks.
- Harder: configuration normalization must be well-defined per resource kind; a config typo yields a new pool rather than a warning (so `inspect` should show identity changes on activation).
- Forbidden: reuse by name alone; mutating a live manager's configuration in place.

## Alternatives considered

- **Reuse by name.** Rejected: unsafe.
- **Never reuse; new managers per revision.** Rejected: wastes connections and defeats drain-time overlap.
