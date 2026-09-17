# Open Design Questions

> Open is a transient state, not a museum. A question stays here only until reasoning, a prototype, or an experiment can close it with an ADR.

## Workflow

```text
new question
   ↓
can reasoning settle it responsibly?
   ├─ yes → ADR now
   └─ no
       ↓
   can an ordinary prototype / benchmark settle it?
       ├─ yes → prototype, then ADR
       └─ no, or high-stakes
             ↓
        formal experiment (in the research repo)
             ↓
             ADR
```

Design choices are closed early where reasoning is sufficient. Empirical questions stay open until reality answers; code avoids locking them in. An agent that must pick an interim answer to proceed states it in the commit **and** in the table below.

## Open

| # | Question | Kind | Interim assumption in code |
|---|---|---|---|
| Q8 | **Multi-core model** — one runtime with shared resources? sharded engines? scheduling topology? | empirical | single-core execution; budgets (ADR-0012) and resource identity (ADR-0011) written so they can be partitioned; no process-global singleton assumptions |
| Q10 | **Absolute p50 gap above c ≈ 16** (EXP-012B) — mechanism unknown | empirical | non-blocking until a target SLO needs it; do not rerun EXP-012B |

## Closed

| # | Question | Closed by |
|---|---|---|
| Q1 | Schema integration | [ADR-0002](adr/0002-schema-strategy.md) |
| Q2 | Response API | [ADR-0003](adr/0003-response-api.md) |
| Q3 | Authentication model | [ADR-0004](adr/0004-authentication-boundary.md) |
| Q4 | Application artifact | [ADR-0005](adr/0005-application-artifact-identity.md) (identity/layering; byte format is a D3 follow-up) |
| Q5 | Development reload | [ADR-0006](adr/0006-revision-lifecycle-and-dev-reload.md) |
| Q6 | Runtime-local state | [ADR-0007](adr/0007-no-runtime-local-persistent-objects.md) |
| Q7 | Revision lifecycle | [ADR-0006](adr/0006-revision-lifecycle-and-dev-reload.md) |
| Q9 | Security boundary | [ADR-0008](adr/0008-security-boundary-baseline.md) (baseline; threat model at D13) |
| Q11 | How the application becomes a definition | [ADR-0009](adr/0009-build-time-application-definition.md) |
| Q12 | Task durability | [ADR-0010](adr/0010-task-lifetime-is-not-durability.md) |
| Q13 | Resource scope across revisions | [ADR-0011](adr/0011-resource-identity-and-reuse.md) |
| Q14 | Admission / backpressure | [ADR-0012](adr/0012-hierarchical-admission.md) |
| Q15 | JS / Web API surface | [ADR-0013](adr/0013-capability-based-api-surface.md) |
| Q16 | Retry / failure semantics | [ADR-0014](adr/0014-no-implicit-retry.md) |

Q1–Q9 originate in `GOAL.md` §56; Q11–Q16 were raised during the 2026-09-17 design review.
