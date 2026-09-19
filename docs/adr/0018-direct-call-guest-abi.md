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

**Three core exports, ours
(`crates/usai-runtime/guest/patches/usai-direct-call.patch`), carry the
request path; nothing is parsed, compiled, allocated or freed per call on
either side:**

- `qjs_usai_inbuf(size) -> ptr`: a buffer the core owns, grown on demand;
  the host writes `name\0arg` into it (one call when it must grow, none
  otherwise);
- `qjs_usai_enter(name, name_len, arg, arg_len) -> (len << 32) | ptr`: calls
  `__usai[name](arg)` with one string and leaves the result text in memory
  the core owns until the next result; bit 63 marks an exception's text. The
  host reads it straight from linear memory;
- `qjs_usai_settle() -> (len << 32) | ptr`: runs the job queue to quiescence
  inside the core and returns `__usai.state()` (the outcome and the pending
  operations) in the same call.

The bridge functions the host enters: `entry("<seed>␟<profiling>␟<index>␟<input
JSON>")` seeds the world and starts the handler; `state()` reports the outcome
and the pending count; `cancel(reason)` and `stop(reason)`. A hello request
is four typed calls into the guest — reseed, `inbuf`, `enter`, `settle` —
where 0.0.5 made three evals plus some thirty helper calls (`wasm_malloc`,
`qjs_get_string`, `qjs_free_*`, one `qjs_execute_pending_job` per microtask,
…) and looked each export up again in every world. The typed handles of
these exports are resolved once per world and kept (the R3 lesson: typed
exports resolved once, not `Func::typed` per call), and the state a settle
returns is kept by the world until the next call into the guest, so the
driver's `state()` never re-enters an idle guest.

`qjs_eval` stays for what is not a request: evaluating the application
module and warming the validators when the image is built, and tests. The
native reference engine (ADR-0015) implements the same trait through direct
`rquickjs` function calls, so both engines run the same bridge functions and
the acceptance suites on both prove the change did not alter behaviour.

The SDK↔bridge surface (`GUEST_ABI` stamped in `builtWith.abi`) is unchanged:
an artifact built by the 0.0.5 SDK runs on this runtime. What changed is how
the host reaches the bridge, which is the runtime's own business.

## What the invoice said

First cut (direct calls, still one helper call per step) on the developer
machine (`GET /zod/:name`, ms per request, same run conditions):
`invoke.eval` 0.28 → `invoke.entry` 0.23 with the SDK's dispatch inside it,
`outcome.eval + pending.eval` 0.15 → `state` 0.10. The parse/compile of the
snippet was smaller than assumed; most of what the ledger had called "the
eval floor" was the SDK's own dispatch (a `flatten(app)` per request, now
computed once per image — C13) and Zod's first accepting parse per world
(now warmed in the snapshot, lever C2a). `perf` on the VM then showed the
per-world export lookups and per-call type checks (`func_loc`,
`RegisteredType::root`, ≈4 % of the request) and the thirty-odd
host↔guest transitions, which the owned buffers and the in-core settle
remove. The full before/after is in the P8 parity report.

## Consequences

- Easier: the request path has no script compilation, no formatted code, no
  quoting of the input JSON into a string literal; one fewer guest call per
  driver iteration.
- Harder: nothing for applications; the core carries three more Usai exports
  (`guest/PROVENANCE.md`); a result text is valid only until the next result,
  which the host honours by copying it out before the next call.
- Forbidden: evaluating code on the request path again; per-request rebuilds
  of definition-lifetime structures in the SDK (C13 applies inside the world
  too).
