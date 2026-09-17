# ADR-0010: Task dispatch transfers lifetime ownership; it does not promise durability

**Status:** accepted (v0)
**Date:** 2026-09-17
**Closes:** Q12

## Context

`ctx.tasks.dispatch(task, input)` transfers ownership from the parent world to the task runtime (C3). Developers may assume "dispatched" means "will eventually run even if the process dies". The runtime has no durable substrate in v0.

## Decision

```text
dispatch  =  the runtime owns the task's lifetime independently of the parent world
dispatch  ≠  durable delivery
```

- A locally dispatched task that has not completed when the process crashes **may be lost**. This is documented behavior, surfaced in `inspect` (delivery: local, non-durable).
- Durability, when needed, is an explicit, separately named substrate (e.g. a queue-backed or `durableTask(...)` variant), declared by the developer, with its own delivery guarantee stated by that substrate.
- Lifetime and durability are orthogonal axes and must never be conflated in API naming or docs.

Principle: **independent lifetime does not imply durable existence.**

## Consequences

- Easier: v0 tasks are simple and fast; no hidden persistence layer.
- Harder: developers must choose durability deliberately; docs must be blunt about crash loss.
- Forbidden: silently persisting dispatched tasks; naming a non-durable dispatch in a way that suggests guarantees.

## Alternatives considered

- **Durable by default (persist every dispatch).** Rejected: forces a storage dependency into the core and hides cost.
