# ADR-0008: Security boundary baseline — semantic isolation is not a hostile-code sandbox; v0 is one trust domain

**Status:** accepted (v0 baseline; threat model deferred to D13)
**Date:** 2026-09-17
**Closes:** Q9 (baseline wording and scope; full threat model remains D13 work)

## Context

Fresh execution worlds give strong *semantic* isolation (state does not leak between units of work). It is tempting to describe that as a security sandbox. The research repo is explicit that adversarial isolation was never tested (`RESEARCH-STATE.md` §8).

## Decision

- Fresh-world isolation is a **correctness/lifecycle guarantee**, not a security boundary. Documentation, code comments, and error messages must not call it a sandbox.
- v0 runtime is **one trust domain**: all applications and all code in a runtime are assumed to be trusted by the operator.
- Any stronger isolation (per-application, per-tenant, hostile code) must be an explicit, named feature with its own threat model and evidence before it is claimed. Introducing one is a stop-and-surface decision (`AGENTS.md` §3).

## Consequences

- Easier: honest scope; no accidental security promises; engine choices are not constrained by an unproven sandbox requirement.
- Harder: multi-application shared runtimes (a stated long-term hypothesis) need a later ADR + threat model before untrusted co-tenancy.
- Forbidden: the words "sandbox", "secure isolation", or "tenant isolation" in public docs for v0.
- Revisit: D13 production hardening — write the threat model then.

## Alternatives considered

- **Claim Wasm isolation as a sandbox now.** Rejected: untested, and the host import surface (capabilities, resources) is where real risk lives.
