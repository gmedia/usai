# ADR-0001: Inherit contracts and lessons from the research repository, not its code

**Status:** accepted
**Date:** 2026-09-17
**Closes:** —

## Context

The research repository (private) holds ~35 experiment crates, a Makefile of ~100 experiment targets, benchmark controllers, judges, freeze/seal machinery, and sealed evidence. Its code was written to make one-shot measurements trustworthy: source-scanning guard tests, pinned SHA-256 constants, evidence builders, registered-string classifications. That structure is correct for research and wrong for a maintainable, developer-facing runtime.

EXP-012B established that the lifecycle model is worth building (`docs/RESEARCH-REFERENCE.md`). It did not establish that the R3 candidate crate is good product architecture.

## Decision

The production repository starts clean. It inherits:

- the lifetime model and the contracts in `docs/LIFECYCLE-CONTRACTS.md`;
- the implementation lessons (bounded ledger, definition-lifetime reuse, lazy export resolution, zero-cost disabled observability, pagemap-aware reset, pooling + COW as the current physical representation);
- the PostgreSQL ownership/cancellation/quarantine contract;
- the claims discipline (what may and may not be said).

It does **not** inherit: experiment crates, harness/controller/judge scripts, freeze/one-shot ceremony, evidence schemas, guard-by-source-scan tests, research-era naming, or the research Makefile. There is no build or runtime dependency on the research repository in either direction.

The research repository remains the immutable evidence record. Decision-critical questions that need a canonical study are run **there**, under its `AGENTS.md`, and their results are referenced from here.

## Consequences

- Easier: a small, readable runtime; freedom to redesign mechanisms as long as the contracts hold; normal iteration speed.
- Harder: nothing is "already done" — every mechanism is re-implemented and re-tested here. Agents must read the research code for understanding, then write production code fresh.
- Forbidden: copying files from `experiments/` into `crates/`; importing research crates; citing development benchmarks here as canonical evidence.
- Revisit: if a production question becomes decision-critical and un-answerable by profiling, open a canonical study in the research repo rather than adding ceremony here.

## Alternatives considered

- **Fork the research repo and prune.** Rejected: the ceremony is woven through the code (guard tests, pinned digests, evidence builders); pruning would take longer than rewriting and would leave research vocabulary in the product.
- **Keep both in one repository with a `research/` directory.** Rejected: sealed-history immutability rules and product iteration speed conflict in one tree; mixed modes were already a documented hazard in the research `AGENTS.md`.
