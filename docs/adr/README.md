# Architecture Decision Records

One file per decision, numbered, never deleted. A superseded ADR stays and gets a `Superseded by` line.

"Accepted (v0)" means: decided by reasoning for the first developer preview, deliberately revisable when implementation or measurement disagrees. It is a stance, not a law.

When to write one: any decision in the stop-and-surface list of `AGENTS.md` §3, or the closing of a row in `docs/OPEN-QUESTIONS.md`. Ordinary local engineering choices do not need an ADR.

Filename: `NNNN-short-kebab-title.md`.

Template:

```markdown
# ADR-NNNN: Title

**Status:** proposed | accepted (v0) | superseded by ADR-NNNN
**Date:** YYYY-MM-DD
**Closes:** Qn in docs/OPEN-QUESTIONS.md (if any)

## Context
What forces the decision. Cite evidence (research file, profile, test) rather than intuition.

## Decision
One paragraph. What we will do.

## Consequences
What becomes easier, what becomes harder, what is now forbidden, what must be revisited and when.

## Alternatives considered
Briefly, with the reason each was not chosen.
```

## Index

| ADR | Title | Status | Closes |
|---|---|---|---|
| [0001](0001-inherit-contracts-not-code.md) | Inherit contracts and lessons from the research repository, not its code | accepted | — |
| [0002](0002-schema-strategy.md) | Standard Schema for validation; JSON Schema as separate metadata capability; no built-in schema | accepted (v0) | Q1 |
| [0003](0003-response-api.md) | Plain return, explicit helpers, raw `Response` escape hatch | accepted (v0) | Q2 |
| [0004](0004-authentication-boundary.md) | Authentication is a declared boundary before admission | accepted (v0) | Q3 |
| [0005](0005-application-artifact-identity.md) | Logical `.usai` artifact identity; engine output is host cache | accepted (v0, format deferred) | Q4 |
| [0006](0006-revision-lifecycle-and-dev-reload.md) | `installed → active → draining → retired`; dev reload = revision replacement | accepted (v0) | Q5, Q7 |
| [0007](0007-no-runtime-local-persistent-objects.md) | No arbitrary runtime-local persistent object primitive | accepted (v0) | Q6 |
| [0008](0008-security-boundary-baseline.md) | Semantic isolation ≠ sandbox; v0 is one trust domain | accepted (baseline) | Q9 |
| [0009](0009-build-time-application-definition.md) | ApplicationDefinition is built, not discovered at runtime | accepted (v0) | Q11 |
| [0010](0010-task-lifetime-is-not-durability.md) | Task dispatch transfers lifetime, not durability | accepted (v0) | Q12 |
| [0011](0011-resource-identity-and-reuse.md) | Resource identity = kind + name + config fingerprint + compat version | accepted (v0) | Q13 |
| [0012](0012-hierarchical-admission.md) | Hierarchical admission budgets, no single global knob | accepted (baseline) | Q14 |
| [0013](0013-capability-based-api-surface.md) | Capability-based minimal Web-like surface; no Node target | accepted (v0) | Q15 |
| [0014](0014-no-implicit-retry.md) | No implicit retry; failure terminal by default | accepted (v0) | Q16 |
| [0015](0015-execution-substrate-quickjs-native.md) | Bootstrap/reference substrate is native QuickJS-ng behind an engine boundary | accepted as scaffolding; production superseded by 0016 | — |
| [0016](0016-wasm-substrate-preinitialized-image.md) | Production substrate: Wasmtime + pre-initialized QuickJS image, pooling + COW | accepted (v0), production evidence pending | ADR-0015 revisit |
