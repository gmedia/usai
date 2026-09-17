# ADR-0004: Authentication is a declared boundary, not a middleware chain

**Status:** accepted (v0)
**Date:** 2026-09-17
**Closes:** Q3

## Context

`GOAL.md` §11 places the auth boundary before admission so that rejected requests do not create a world (C6). The open question was the shape: reusable auth workload, per-route declaration, how the principal enters the world, and how much reaches OpenAPI.

## Decision

Authentication is a **reusable, declared boundary** referenced per workload:

```ts
const authenticated = auth.bearer({ resolve: async (ctx, token) => findUser(token) });

http.get("/me", { auth: authenticated, response: User }, async (ctx) => ctx.auth);
```

Pipeline order is fixed:

```text
request → structural validation → authentication → admission → world → handler
```

The resolved principal is typed and enters the world as `ctx.auth`. Authentication (who) is the runtime's boundary; **authorization** (may they) is business logic and stays in the application (C7). Auth declarations are part of the ApplicationDefinition and therefore visible to `inspect` and OpenAPI (security schemes), without leaking resolver internals.

## Consequences

- Easier: one auth declaration reused across routes; rejected auth never pays for a world; docs know which routes are protected.
- Harder: the resolver runs before a world exists, so it must be defined as runtime-side work with its own ownership and cancellation rules; where exactly the world is created relative to the resolver is an optimization the runtime may tune.
- Forbidden: generic ordered middleware chains as the primary model; business permission checks inside the auth boundary.

## Alternatives considered

- **Express-style middleware.** Rejected: hidden ordering, no declaration in the definition, cannot be inspected or documented.
- **Auth inside the handler.** Rejected: violates C6 (world created for work that is then rejected).
