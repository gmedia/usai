# ADR-0014: No implicit retry; failure is terminal by default; retry is explicit per workload and named by its substrate

**Status:** accepted (v0)
**Date:** 2026-09-17
**Closes:** Q16

## Context

Automatic retry hides at-least-once semantics from developers and can multiply side effects. Different workload substrates have genuinely different delivery semantics (a cron tick vs a queue message).

## Decision

Default for every workload:

```text
one trigger → one invocation → success or terminal failure
```

- Cron failure: a logged failed invocation; the next scheduled tick is unaffected.
- Task failure: terminal; reported to the dispatcher's observability, not retried.
- HTTP failure: an error response.
- Queue consumers may declare retry because the message substrate has that semantic, and only explicitly:

```ts
queue.consume("orders", { retry: { maxAttempts: 5, backoff: "exponential" } }, handler);
```

The declaring developer accepts at-least-once implications; the docs for that workload state the delivery guarantee the substrate provides.

## Consequences

- Easier: side effects run once unless the developer said otherwise; failure semantics are visible in `inspect`.
- Harder: transient failures in tasks/cron need explicit handling by the application or a future explicit retry declaration.
- Forbidden: runtime-level retry loops around handlers; retry declarations without a stated delivery guarantee.

## Alternatives considered

- **Retry transient errors automatically.** Rejected: the runtime cannot know which side effects are idempotent.
