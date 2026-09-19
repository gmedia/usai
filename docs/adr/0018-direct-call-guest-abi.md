# ADR-0018: The host enters the guest by calling, not by evaluating

**Status:** accepted (v0)
**Date:** 2026-09-19
**Closes:** the "core ABI — stop-and-surface" lever left open by the P1/P2 attribution (`docs/measurements/2026-09-18-execution-path-attribution.md`, lever #5) and `docs/STATUS.md`'s "eval-based invoke/outcome/pending floor"

## Context

Through 0.0.5 the Wasm engine entered every world by formatting a script —
`__usai.seed("…");__usai.invoke(i, <input JSON>)` — and running the core's
`qjs_eval` on it, then two more evals per driver iteration to read the
outcome and the pending count. It was the simplest thing that worked when
the substrate arrived (ADR-0016) and it was known to be a cost: the
attribution ledger put the three evals at 0.24 ms of the 1.05 ms hello
request. The research representation (R3) never evaluated on the hot path;
it called typed exports (`docs/RESEARCH-REFERENCE.md`).

P8 (`docs/ROADMAP.md`) opened with the whole-request invoice
(`USAI_PROFILE=1`, `profile_matrix http_invoice`) and asked what the
implementation pays that the semantics never asked for.

## Decision

**One core export, `qjs_usai_call(name, name_len, arg, arg_len) -> JSValue*`,
calls `__usai[name](arg)` with one string and returns the result like
`qjs_eval` does** (`crates/usai-runtime/guest/patches/usai-direct-call.patch`).
The host enters the guest through it for everything on the request path:

- `entry("<seed>␟<profiling>␟<index>␟<input JSON>")` seeds the world and
  starts the handler (one call where there were an eval of a formatted
  script);
- `state()` returns the outcome and the pending count together (one call per
  driver iteration where there were two evals; the driver keeps the settled
  state instead of reading it twice);
- `cancel(reason)` and `stop(reason)` are called, not evaluated.

`qjs_eval` stays for what is not a request: evaluating the application
module and warming the validators when the image is built, and tests. The
native reference engine (ADR-0015) implements the same trait through direct
`rquickjs` function calls, so both engines run the same bridge functions and
the acceptance suites on both prove the change did not alter behaviour.

The SDK↔bridge surface (`GUEST_ABI` stamped in `builtWith.abi`) is unchanged:
an artifact built by the 0.0.5 SDK runs on this runtime. What changed is how
the host reaches the bridge, which is the runtime's own business.

## What the invoice said

Before → after on the developer machine (`GET /zod/:name`, ms per request,
same run conditions): `invoke.eval` 0.28 → `invoke.entry` 0.23 with the SDK's
dispatch inside it, `outcome.eval + pending.eval` 0.15 → `state` 0.10. The
parse/compile of the snippet was smaller than assumed; most of what the
ledger had called "the eval floor" was the SDK's own dispatch (a
`flatten(app)` per request, now computed once per image — C13) and Zod's
first accepting parse per world (now warmed in the snapshot, lever C2a).
Together the three levers took the hello request from 1.70 to ≈1.0 ms on the
VM at c=1 with the soak as a co-tenant; the full before/after is in the P8
parity report.

## Consequences

- Easier: the request path has no script compilation, no formatted code, no
  quoting of the input JSON into a string literal; one fewer guest call per
  driver iteration.
- Harder: nothing for applications; the core carries one more Usai export
  (`guest/PROVENANCE.md`).
- Forbidden: evaluating code on the request path again; per-request rebuilds
  of definition-lifetime structures in the SDK (C13 applies inside the world
  too).
