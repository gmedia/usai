# Idle decommit: what it reclaims here, and why that is not the plateau

**2026-09-26, VM 47 (16 vCPU, Linux 6.18), release build at `07073c0`.**
Contract-heavy class (`POST /orders/quote` of the qualification bench app:
validate 12 fields, compute, encode), 32 keep-alive clients, 8 s, then idle.
`--max-worlds 48`. Raw runs and the script are in
`docs/measurements/data/2026-09-26-idle-decommit/`.

## What this was meant to settle

ADR-0019 lever 2 says the operator-visible cost of this runtime is
per-concurrency memory: *"a world slot keeps up to `USAI_WASM_KEEP_RESIDENT`
(8 MiB) after use, so the plateau is the peak concurrency ever seen, not the
current load — 64 concurrent requests cost half a gigabyte and keep it."* The
fix it proposed was an embedder-driven release, which needed an upstream API;
that API is now merged
([wasmtime#14419](https://github.com/bytecodealliance/wasmtime/pull/14419)),
vendored (`vendor/README.md`), and wired to the epoch ticker.

This is the before/after the ADR asked for. It does not say what the ADR
expected.

## Measured

| | post-burst RSS | post-burst PSS | warm-unused, accounted | released | RSS after release |
|---|---|---|---|---|---|
| `keep_resident=8m`, release off | 111.2 MiB | 63.3 MiB | 12.2 MiB | — | — |
| `keep_resident=8m`, release forced (floor 0) | 111.2 MiB | 63.1 MiB | 12.2 MiB | 12.2 MiB | **111.4 MiB** |
| the same, repeated | 111.5 MiB | 63.0 MiB | 12.2 MiB | 12.2 MiB | **111.6 MiB** |
| `keep_resident=0`, release off | **50.5 MiB** | 48.0 MiB | 0.0 MiB | — | — |

Baseline before each burst: RSS ≈ 45.5 MiB, PSS ≈ 43.0 MiB.

Throughput over the burst: 20 245–20 515 requests at `keep_resident=8m`
against 17 142 at `0`, i.e. the documented ≈16 % the keep-resident trade buys
on this box at this concurrency.

## What it says

**The 60 MiB of post-burst RSS is keep-resident memory** — the
`keep_resident=0` row proves it, and pays 16 % of throughput to avoid it.

**But only 12.2 MiB of it is reachable by this API**, and releasing that
12.2 MiB does not lower RSS at all (111.2 → 111.4, and 111.5 → 111.6 on the
repeat: no change, twice). The release ran — `releases=1`,
`usai_pool_memory_released_bytes_total` moved, the plateau gauge went to zero —
so this is not a policy that failed to fire.

Two things account for the gap, and both are ours rather than upstream's:

1. **`max_unused_warm_slots(0)`**, which this runtime sets deliberately: the
   P6 churn campaign OOM'd after ~270 revision replacements because Wasmtime's
   default keeps up to 100 unused warm slots and prefers *cold* slots for a new
   module. With 0, a freed slot is re-picked rather than accumulated, so warm
   slots — the only thing `release_idle_pool_memory` can reach — barely
   accumulate. **The plateau lever 2 targets was already closed, by that.**

2. **The rest of the RSS is per-slot copy-on-write image pages.** The 48 MiB
   gap between RSS (111) and PSS (63) is that signature: one set of physical
   pages mapped into every slot, counted once per mapping by the kernel's RSS
   accounting. `keep_resident` holds them by resetting in place instead of
   madvising; the allocator does not count them in the `bytes_resident` it
   tracks per slot, so the release does not touch them.

## What was done about it

The implementation is kept and **switched off by default**
(`USAI_IDLE_DECOMMIT_MS=0`). Shipping it on would ship a claim this page
contradicts. What is kept unconditionally is the gauge —
`usai_pool_memory_resident_unused_bytes` — because the interesting fact here
is how *small* it is next to RSS, and an operator sizing a box should be able
to see that rather than infer it.

ADR-0019's lever 2 is amended rather than closed: the per-concurrency cost is
real (the `keep_resident=0` row), but it is not a warm-slot plateau and the
upstream API is not the lever for it. What would be:

- accepting the 16 % and running `USAI_WASM_KEEP_RESIDENT=0` on
  memory-bound boxes, which is already documented
  (`runbooks/memory-pressure.md`) and is the only measured way to get the
  60 MiB back today;
- shrinking the image itself, which is lever 3 (the world's entry cost) and
  reduces the per-slot COW set for every slot at once;
- an upstream change that releases a slot's *image* pages, not only its
  accounted dirty bytes — a different proposal from #14419, and one that would
  need the same measurement before it is worth a maintainer's time.

## What this does not measure

Whether the release helps an embedder that leaves `max_unused_warm_slots` at
its default. It does: the standalone program in
`docs/upstream/idle-decommit-measurement/` gives back 256 MiB of 266.9 MiB RSS
under exactly those settings. That is the configuration the upstream API was
proposed for, and the numbers there stand. This page is about ours.
