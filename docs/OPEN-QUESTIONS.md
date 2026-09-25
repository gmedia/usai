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
| Q10 | **Absolute p50 gap above c ≈ 16** (EXP-012B) — mechanism unknown. **Priced on production code, 2026-09-23**: the sweep (`2026-09-23-sweep.md`) and the saturation run to c=512 (`2026-09-23-saturation-and-queue.md`) measure the gap at every concurrency, and its *shape* is a constant per-request cost rather than a divergence with load — throughput stays flat from c=64 to c=256 and latency grows with the concurrency exactly as a saturated queue does, on this runtime and on every comparator. That makes it Q18's question (the interpreter) rather than a scaling defect | empirical | non-blocking until a target SLO needs it; do not rerun EXP-012B |
| Q17 | **Multi-application runtime process** — one process, one pool, many definitions: the only way to remove the per-application intercept the P8E density run measured (≈30 MiB PSS and one process per idle application, linear to N=50; `docs/measurements/2026-09-20-p8e-efficiency.md` §5, §8). `GOAL.md` §52 lists the memory reservation policy for many-app density as research-grade | formal research | one process per application; revisions are of one application (`Runtime.active`); density is processes per host and `SUPPORTED.md` states its arithmetic (N × `pool.max` connections) |

| Q18 | **The interpreter is 42 % of a trivial request** — the sweep prices the model at 6–14× Node's CPU on a route where nothing else dominates (`docs/measurements/2026-09-23-sweep.md`), and the invoice says most of that is QuickJS interpreting the application's own JavaScript. The two ways out both cost something the runtime is built on: a JIT inside the guest makes compiled code that survives a world (state a fresh world must not inherit), and compiling the application to WebAssembly ahead of time (Javy/Porffor-shaped) is a semantics risk and a research programme. Neither is in P9 (ADR-0019) | formal research | the interpreter stays; the fresh world is cheap to create and reset *because* nothing is compiled per world, and the sweep shows the cost disappears on any route that touches PostgreSQL |

## Closed

| # | Question | Closed by |
|---|---|---|
| Q1 | Schema integration | [ADR-0002](adr/0002-schema-strategy.md) |
| Q2 | Response API | [ADR-0003](adr/0003-response-api.md) |
| Q3 | Authentication model | [ADR-0004](adr/0004-authentication-boundary.md) |
| Q4 | Application artifact | [ADR-0005](adr/0005-application-artifact-identity.md) (identity/layering; byte format decided by ADR-0020: the manifest is the compatibility surface, the rest of the directory is a build output and not an interchange format) |
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
| Q19 | Readiness vs a shared dependency | [ADR-0022](adr/0022-readiness-does-not-follow-a-shared-dependency.md) — readiness reports resource health rather than following it; `USAI_READY_REQUIRES_RESOURCES=1` couples them for a replica that owns its database |
| Q20 | Unreachable dependency at activation | [ADR-0023](adr/0023-activation-retries-a-dependency.md) — retried for 60 s by default, then activation fails as before; a missing variable is never retried; `USAI_ACTIVATION_RETRY=0` restores the immediate exit |
| Q21 | Outbound protocols beyond HTTP | [ADR-0024](adr/0024-outbound-io-is-http.md) — outbound I/O is HTTP; other protocols are reached through a sidecar that exposes them over HTTP, which the documents now name as the supported shape |

Q1–Q9 originate in `GOAL.md` §56; Q11–Q16 were raised during the 2026-09-17 design review.
