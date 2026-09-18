# Research Reference

> Map from production concepts to the research evidence that justifies them.

This is the "research-reference document" required by `GOAL.md` D0. It tells a maintainer **where to look** in the research repository and **what each result is allowed to mean**. The research repository is **private**; readers without access should rely on `docs/LIFECYCLE-CONTRACTS.md`, the ADRs and `docs/measurements/` in this repository, which carry every conclusion the production code depends on. It does not restate the results; the sealed documents are authoritative.

## The research repository

```text
Access        private — Sakala maintainers (ask a maintainer for the checkout)
Referenced at c1a57a8ba310488a055613471481d183a851eace  (2026-09-17)
Sealed head   EXP-012B, seal commit 80830805c3cf9d761bd1fe976fd8888299fc2ec7
```

Rules when reading it:

- `docs/experiments/*-RESULT.md`, `*-RESULTS.md`, `*-FREEZE.json`, and `artifacts/` are **sealed**. Never edit, even to fix a typo. Corrections are new documents there, not here.
- A sealed result means exactly its registered scope. Do not widen it.
- Experiment code is **evidence machinery**, not a template. Read it for the mechanism; do not port its harness, evidence schemas, freeze checks, or source-scanning guard tests.
- Research-era names (R1/R2/R3, incarnation, Phase S, exp011d, …) must not leak into production code or developer-facing docs. `docs/GLOSSARY.md` translates them.

## Start here

| Need | File in research repo |
|---|---|
| What the evidence allows us to believe / must not claim | `docs/RESEARCH-STATE.md` |
| Concise handoff into the development phase | `docs/CONTEXT_UPDATE_2026-09-17_POST_EXP012B.md` |
| Latest sealed result | `docs/experiments/EXP-012B-RESULT.md` |
| Its preregistration (defines the five R3 differences, §4.1–4.5) | `docs/experiments/EXP-012B.md` |
| Module map of the R3 candidate | `docs/experiments/EXP-012B-IMPLEMENTATION-NOTE.md` §2 |
| Research working agreement (for context on why the ceremony existed) | `AGENTS.md`, `CLAUDE.md` |
| Hot-path audit checklist | `docs/IMPLEMENTATION-ECONOMICS-AUDIT.md` |
| Validation questions still open | `docs/RESEARCH_AGENDA.md` |
| Name rationale | `docs/NAMING.md` |

## Contract → evidence

| Production contract (`LIFECYCLE-CONTRACTS.md`) | Established by | Sealed verdict |
|---|---|---|
| C1 five lifetimes; definition reused across worlds | EXP-005B/006 lineage, EXP-007, EXP-011B, EXP-012B §4.2 | pooling + COW representation; `R1_ECONOMICS_MATERIAL` (011B) |
| C1 world isolation at concurrency | EXP-012B Phase C | 64 simultaneous worlds, world 65 refused, 16 aborts / 48 survivors, C1–C6 HOLD |
| C4 death ≠ cleanup; identity-first completion gate | EXP-009C, EXP-011C, EXP-012B Phase S | `CANCELLATION_MODEL_SUPPORTED`; 14/14 stress oracles HOLD |
| C4 bounded live ownership ledger | EXP-012B §4.1 (Amendment 2) | growth in time **and** space is a defect |
| C5 PostgreSQL terminal-proof / quarantine | EXP-010A | `PG_CAPABILITY_COOPERATIVE_ONLY` |
| C5 persistent capability outliving worlds | EXP-009A | `CAPABILITY_BOUNDARY_ACCEPTABLE` |
| async suspend/resume without thread-per-world | EXP-009B | `ASYNC_EXECUTION_ACCEPTABLE` |
| C12 disabled observability must be free | EXP-012B §4.4 | non-recording event path |
| C13 costs not to re-pay | EXP-010B-R2 → 010C-R2 → 011D → 011E → 012A-R1 → 012B | see below |
| representation lesson: V8 snapshot does not fix context cost | EXP-004B | `V8-SNAPSHOT → REJECTED_AS_LIFECYCLE_FIX` |
| representation lesson: bounded VA reservation regresses memory-active JS | EXP-007 | `BOUNDED_VA_TOO_EXPENSIVE` (VA policy remains open debt) |

## The economics lineage in one screen

```text
EXP-010B-R2   HTTP_COMPOSITION_TOO_EXPENSIVE        real HTTP+PG, R1-era host: negative
EXP-010C-R2   COST_ATTRIBUTION_LIFECYCLE_DOMINANT   the tax was in lifecycle/host work
EXP-011D      R1_HTTP_ECONOMICS_INSUFFICIENT
EXP-011E      R2_HTTP_ECONOMICS_MATERIAL at c = 1
EXP-012A      HARNESS_INVALID / NO_VERDICT          harness defect, attempt spent
EXP-012A-R1   R2_CONCURRENCY_BOUNDARY_FOUND         first fail c = 1, no passing level
EXP-012B      R3_CONCURRENCY_BOUNDARY_FOUND         full envelope pass through c = 16,
                                                    fail at c = 32/64 by absolute p50 delta only,
                                                    all ratio gates pass through c = 64,
                                                    126/126 cells, 1.26M requests, 0 failures
```

R2 → R3 was exactly five host-side changes (EXP-012B §4):

```text
4.1  indexed physical-operation ledger with live/history lifetimes separated
4.2  owned, immutable ApplicationDefinition holding InstancePre + export indices
4.3  lazy per-world typed export resolution
4.4  disabled semantic-event path that neither allocates nor formats
4.5  PAGEMAP_SCAN-aware linear-memory reset
```

Effect as a bundle (not attributed individually):

```text
             R2 (012A-R1)   R3 (012B)
CPU ratio c1     2.080        1.096
CPU ratio c64    4.382        1.205
p50 ratio c1     1.755        1.123
p50 ratio c64    4.915        1.340
```

The lesson this repo inherits: **an expensive lifecycle can be an implementation problem rather than a semantic law.** Profile before blaming the model.

Open from EXP-012B and still unexplained: why the absolute p50 gap grows past c ≈ 16 while all ratios stay in bounds. Investigate when it blocks a target SLO; do not rerun EXP-012B.

## Where the R3 mechanism lives (read for design, not for copying)

`experiments/exp011d-econ/src/` in the research repo:

| File | What to learn from it |
|---|---|
| `world.rs` | `WorldDriver`: sole owner of the Wasmtime `Store`; `enter_wasm`/`leave_wasm` reentrancy guard; the single completion `route()` gate (incarnation check → revoked check → registry take → export); `retire()` as the one teardown path; `Drop` retires abandoned worlds without entering Wasm |
| `guest.rs` | `ApplicationDefinition { module, pre: InstancePre, exports }`; host imports copy guest bytes before anything else; `start_database_operation` spawns an owner that can never reach the Store |
| `physical.rs` | `PgCapability`, `Lease` (reusable only via `mark_reusable` after terminal proof; `Drop` quarantines otherwise), `PhysicalState`, `ReleaseDecision`, the two-lifetime `Records` ledger |
| `logical.rs` | `LogicalRegistry`, `CompletionEnvelope`, `RouteDecision` — logical cancellation by incarnation |
| `engine.rs` | pooling allocator config: `memory_init_cow`, `total_*` = concurrency, `max_memory_size`, keep-resident tuning, `pagemap_scan` |
| `evidence.rs` | the non-recording builder pattern (`detail_with(|| …)` closures so disabled events pay nothing) |
| `request.rs`, `server.rs`, `http.rs` | how a request became a world and a response in R3 |

Guest side: `fixtures/exp011d/app.js` shows the opaque-capability shape (kind + bounded payload; the guest never sees SQL, connections, or host pointers) and the fresh-baseline mutation witness.

Toolchain used by R3 (a baseline to pin deliberately in this repo, not an inherited pin):

```text
Rust 1.95, edition 2024, clippy -D warnings, rustfmt
Wasmtime 47.0.4 (patched source tree for pagemap_scan), features async + cranelift + runtime + std
wasmtime-wizer 47.0.4 for pre-initialization
QuickJS-ng via quickjs-wasi, WASI SDK 32
tokio, hyper 1 / axum 0.8, deadpool-postgres 0.14, tokio-postgres 0.7
Node 24 for the comparator and harness
```

## What the evidence does not cover

Do not cite research for any of these; they are engineering + validation work in this repo:

```text
multi-core scaling                 long-duration soak              crash / restart recovery
rolling deployment / rollback      backpressure and overload       many-app / idle density
memory fragmentation over time     security threat model           adversarial isolation
artifact reproducibility           Node/npm compatibility budget   operational observability
```
