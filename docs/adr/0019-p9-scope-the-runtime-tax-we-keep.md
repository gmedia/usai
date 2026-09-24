# ADR-0019: The runtime tax we keep, and the three levers worth pulling

**Status:** accepted (v0)
**Date:** 2026-09-23
**Decides:** whether to open P9 (a second hot-path phase) now that its three
inputs exist — the c=1 invoice (`docs/measurements/2026-09-19-p8-parity.md`),
the efficiency envelope (`2026-09-20-p8e-efficiency.md`) and the concurrency
sweep against six comparators (`2026-09-23-sweep.md`).

## Context

The sweep priced the model against everything a team would otherwise deploy,
on one idle 16-thread box, six workload classes, every comparator doing the
same validated work:

| | hello (A) | validation (B) | DB read (C) | insert (D) | transaction (E) | auth+read (F) |
|---|---|---|---|---|---|---|
| Usai, c=64 | 14 843 | 8 614 | 7 023 | 2 385 | 4 216 | 5 344 |
| one Node process | 12 730 | 8 360 | 6 327 | 2 818 | 2 607 | 4 449 |
| Node × 8 (cluster) | 106 449 | 62 101 | 13 971 | 2 680 | 5 145 | 6 797 |
| Bun | 30 322 | 15 227 | 8 785 | 2 652 | 3 949 | 6 395 |
| PHP-FPM tuned (64 children) | 12 684 | 12 643 | 5 908 | 2 385 | 2 738 | 2 961 |
| Laravel 12 | 2 089 | 1 469 | 1 727 | 1 316 | 1 327 | 1 673 |
| axum (Rust) | 164 721 | 164 665 | 25 480 | 2 892 | 7 265 | 12 205 |

CPU per request at c=16 is the number that does not move with the load
shape: Usai 0.52 ms on hello and 0.99–1.66 ms on the database classes,
against Node's 0.08 / 0.16–0.32 and Bun's 0.037 / 0.11–0.29. **Six to
fourteen times Node's CPU on a trivial route, four to six on a database
route.** Memory grows with concurrency, not with request count: ≈0.5 GiB of
resident slots at c=64 where one Node process holds 143 MiB (and a Node
cluster, the comparator that matches the throughput, holds 877 MiB–1 GiB).

The invoice says where that CPU goes on a hello request: **≈42 % executing
the application's JavaScript in the interpreter**, ≈10.5 % resetting the
world's memory, ≈10 % network and syscalls, ≈2.5 % the runtime's own
bookkeeping, and the rest the boundary — decoding the request, validating
declared slots, encoding the response, opening and closing the ownership
ledger.

## Decision

**P9 does not open as a general "make it faster" phase.** Three levers are
worth pulling and they are scoped; everything else on the list is either
research or a different product.

1. **The boundary encoding** (open, P9's only throughput item). Every
   request crosses the host↔guest boundary as JSON text and is parsed on
   the other side, per slot. ADR-0018 removed the *evaluation* on that path;
   the parse remains. A typed encoding — the host handing the guest a
   structured handle it already owns — is the last piece of per-request work
   the semantics never asked for. It costs a `GUEST_ABI` bump and the
   compatibility rules that go with it, so it is a phase, not an afternoon.
2. **Per-concurrency memory** (open, and the one an operator feels first).
   A world slot keeps up to `USAI_WASM_KEEP_RESIDENT` (8 MiB) after use, so
   the plateau is the peak concurrency ever seen, not the current load — 64
   concurrent requests cost half a gigabyte and keep it. An idle-decommit
   policy (return a slot's pages after N seconds unused, pay the faults only
   on the first world after a quiet period) gives a small box both curves.
   Contained, measurable with the P8E cells, no ABI change. **Investigated
   2026-09-23 (`docs/upstream/wasmtime-idle-decommit.md`), open upstream as [#14413](https://github.com/bytecodealliance/wasmtime/issues/14413) (2026-09-24; a first attempt, #14399, was closed under the Bytecode Alliance AI tool policy): the kept region is
   anonymous memory the pool zeroes — not the module's image, which arrives
   through the COW mapping — so releasing it has no correctness consequence.
   But its address belongs to Wasmtime's allocator and `keep_resident` is
   fixed when the engine is built, so this lever is an upstream API plus one
   call from the idle path we already have, not local work.**
3. **The world's entry cost** (open, small). What remains of the reset after
   the upstream Wasmtime patch landed (2026-09-23, merged) is the memcpy of
   the dirty pages the core itself touches. Shrinking the core's own working
   set shrinks every world's entry.

**Not now, and why:**

- **The interpreter** is 42 % of a trivial request and the only lever that
  would change the *class* of the number. The ways to remove it — a JIT
  inside the guest, or compiling the application's JavaScript ahead of time
  into WebAssembly — either break the property the runtime exists for (JIT
  state is state that survives a world) or are a research programme with a
  semantics risk of their own. Recorded in `docs/OPEN-QUESTIONS.md`, not in
  P9.
- **Catching a Node cluster on hello** is not a goal. Eight processes are
  faster at the trivial route and answer the same request differently
  depending on which process receives it (the sweep's counter probe:
  `1 1 1 1 1 1 1 1 2 2`). That is the bug class this runtime exists to
  remove, and it is not purchasable at 7× the throughput.
- **A second process per core** would trade the model for the cluster's
  numbers. One process per application stays the topology (`SUPPORTED.md`).

## Consequences

- The positioning is the measurement, and it is honest in both directions:
  **on a route where nothing else dominates, this runtime costs 4–14× the
  CPU of a bare JavaScript server; on a route that touches PostgreSQL — what
  an application is made of — it is level with a single Node process and
  ahead of it on transactions, ahead of a tuned PHP-FPM, and 1.8–7× ahead of
  Laravel.** Nobody should choose it for hello-world throughput, and the
  README says so.
- **P7 comes first.** The sweep says the runtime is not the bottleneck of a
  real application; the unknown that gates the contracts is whether people
  can build on it (`docs/ROADMAP.md` P7), not whether it can serve another
  three thousand hello requests a second.
- The three levers above get measured before and after with the same
  invoice, on the same VM, or they do not count.
- Anyone reading a benchmark of this runtime should read
  `docs/measurements/BENCHMARKS.md` first: two of the seven comparators in
  the sweep were, in their first run, measuring our own proxy configuration
  rather than the thing they named.
