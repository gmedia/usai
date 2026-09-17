# ADR-0012: Admission and backpressure are hierarchical budgets, not one global concurrency knob

**Status:** accepted (v0 baseline)
**Date:** 2026-09-17
**Closes:** Q14

## Context

EXP-012B showed capacity should be indexed by workload concurrency, not process-global singletons, and that resource limits (a PG pool of N) bound useful concurrency regardless of how many worlds could be created. A single `maxConcurrency` cannot express "HTTP may admit 500 worlds but only 32 can hold a connection".

## Decision

Capacity is a hierarchy; admission consults each level:

```text
runtime capacity        engine/world slots, memory
   ↓
application capacity    per revision
   ↓
workload capacity       per HTTP route group / task / cron / queue consumer
   ↓
resource capacity       pool sizes, leases
```

Work that needs a resource experiences **resource-aware backpressure** (queue with bound, or reject with a typed, documented error) rather than being admitted into a world that then blocks on a lease.

v0 implements the *shape* (budgets are explicit objects that admission checks); a sophisticated scheduler is not required for D1–D2. The abstraction must not make later scheduling impossible.

## Consequences

- Easier: overload behavior is explainable per level; `inspect` can show budgets; multi-core work (Q8) can partition budgets later.
- Harder: more configuration surface; defaults must be sane so a new project needs none.
- Forbidden: a single global concurrency limit as the whole model; admitting work whose required resource is known to be exhausted without a bounded wait policy.

## Alternatives considered

- **Single `maxConcurrency`.** Rejected: cannot express resource-bound capacity.
- **Full scheduler in D1.** Rejected: over-building before workloads exist (`GOAL.md` §18 spirit).
