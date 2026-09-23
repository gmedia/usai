# Upstreaming the Wasmtime pagemap reset patch

Status (2026-09-23): **merged upstream.**
<https://github.com/bytecodealliance/wasmtime/pull/14357> was merged into
`bytecodealliance/wasmtime` `main` on 2026-09-23 00:35 UTC as `a2e2d86`, by
`alexcrichton`, four days after it was opened.

The path from here is mechanical:

1. `main` carries it; **v49.0.0 (2026-09-21) predates the merge**, so no
   released version has it yet. (Re-checked against crates.io on 2026-09-23
   at 20:55 UTC: 49.0.0 is still the newest. Wasmtime releases monthly on the
   20th, so the first candidate is 50.0.0 around 2026-10-20 — this is a
   calendar item, not a task.)
2. The first wasmtime release after 2026-09-23 makes `vendor/` unnecessary:
   delete `vendor/wasmtime` and `vendor/wasmtime-pagemap-reset.patch`, drop
   the `[patch.crates-io]`/`path` entry in the workspace `Cargo.toml`, bump
   the `wasmtime` dependency to that version, and run
   `engine::wasm::tests::a_reused_slot_is_fresh_*` plus the lifecycle and
   HTTP suites on both engines. Those tests are the check that the released
   crate really carries both behaviours.
3. Until then the vendored file stays — and it is byte-identical to upstream
   `main` (checked 2026-09-23), so the eventual swap changes nothing about
   how the runtime behaves.

### The review (2026-09-21 → 09-22)

`alexcrichton` reviewed: "the performance boost and fix here both seem
reasonable to me", and classed the paged-out-dirty-page hole as a bugfix
rather than a CVE because it needs an off-by-default, explicitly unsupported
option. Five comments, all answered on 2026-09-22 (`a327688`): a stray
`pagemap.rs.orig` removed; the documented scan criteria updated (the list no
longer claims `PRESENT`, and the `WRITTEN` bullet carries the swapped-out
case); `remaining_budget` and `pages_found` folded into one `budget` local
that the regions spend. The sixth point was a question — whether a different
buffer size or a dynamically allocated one would be better — and the answer
measured on Linux 6.18 (4 KiB pages, an 8 MiB region, a complete traversal
at several capacities against the cost of resetting the same bytes):

| dirty set | cap 16 | 32 | 64 | 128 | 512 | resetting the same pages |
|---|---|---|---|---|---|---|
| 2048 contiguous pages (1 region) | 1 ioctl, 76 µs | 1, 70 µs | 1, 70 µs | 1, 70 µs | 1, 75 µs | 420 µs |
| every 2nd page (1024 regions) | 64 ioctl, 174 µs | 32, 95 µs | 16, 67 µs | 8, 53 µs | 2, 44 µs | 86 µs |
| every 16th page (128 regions) | 8 ioctl, 26 µs | 4, 18 µs | 2, 14 µs | 1, 9 µs | 1, 14 µs | 9 µs |

An ioctl costs ≈2 µs; a contiguous dirty set is one call at any capacity and
the scan is ≈6× cheaper than the memcpy that resets it. Capacity matters only
for a fragmented set, and even at the every-other-page extreme 32 → 64 saves
≈28 µs against 86 µs of resets, while 64 → 512 saves ≈22 µs more for 12 KiB
of stack (a `page_region` is 24 bytes). With the resume loop the capacity is
a throughput knob, not a correctness one — which is why it stays a fixed
stack buffer of 64.

### What was proposed (kept for the record — written before the merge, tenses unchanged)

The pull request to open on `bytecodealliance/wasmtime` (a fork under the
maintainer's account, branch `pagemap-reset-complete-traversal`), with the
text below. Opening it is a public action on a third-party repository and
is done by a maintainer, not by automation. Until it lands, `vendor/README.md`
is the provenance record and `engine::wasm::tests::a_reused_slot_is_fresh_*`
are the rebase tests: re-vendor, re-apply, run them.

---

## Title

pooling allocator: complete the pagemap scan past 32 regions, and reset dirty pages that are not present

## Body

`reset_with_pagemap` collects dirty regions into a stack buffer of 32 and
treats the kernel's `walk_end` as the end of the memory it will ever reset
in place: everything after it is decommitted with `madvise(DONTNEED)`.

Two consequences we hit running a QuickJS-based guest with a fresh
instance per request (pooling allocator, `memory_init_cow`, `pagemap_scan`):

1. **Throughput.** A JS heap dirties far more than 32 disjoint runs, so
   the scan stopped early on every reset and the rest of the heap was
   decommitted. Each new instance then re-faulted its heap (95–315 minor
   faults per request measured with `perf stat`), and the `madvise` +
   `mprotect` storm capped multi-core scaling with TLB-shootdown IPIs.
   Resuming the scan from `walk_end` whenever the buffer filled, and
   stopping early only when the `keep_resident` page budget is spent (which
   is what `walk_end` is for), brought a request from 2.1 ms to 1.0 ms on
   one core and ×5 at 16 concurrent instances on a 16-core host, with 0
   faults per request.

2. **Freshness.** The mask requires `PRESENT`. A dirty page the kernel has
   swapped out is `WRITTEN | SWAPPED`, not `PRESENT`; it is neither reset in
   place nor decommitted (it sits before `walk_end`), so the next instance in
   the slot reads the previous instance's bytes. We reproduced this with a
   swapfile and `MADV_PAGEOUT` between two instantiations of the same slot:
   the second instance observed the first's heap and trapped in the
   allocator. Requiring `WRITTEN` and not `PFNZERO`/`FILE` regardless of
   residency fixes it — resetting a swapped page pages it in and overwrites
   it, which is the cost of correctness. With 32 regions the accidental
   decommit of everything after `walk_end` hid most of this; with a complete
   traversal it would be exposed on every host with swap, so the two changes
   belong together.

The patch keeps the fixed stack buffer (64 regions) and loops the ioctl,
so there is still no allocation on the reset path. Unit tests in
`pagemap.rs` (they skip where `PAGEMAP_SCAN` is unavailable):

- `reset_resumes_past_the_region_buffer`: 200 disjoint dirty pages in a
  400-page mapping, `keep_resident` = everything — all 200 reset in place,
  nothing decommitted, the mapping reads zero;
- `reset_stops_at_the_page_budget`: same mapping, `keep_resident` = 100
  pages — exactly 100 reset, the rest decommitted;
- `reset_covers_dirty_pages_that_were_paged_out`: 64 dirty pages,
  `MADV_PAGEOUT`, then the reset — every page reset (on a host with swap
  this fails with the old `PRESENT` mask; elsewhere it is the ordinary
  reset).

Measurements and the perf ledger are in the Usai repository
(`docs/measurements/2026-09-18-execution-path-attribution.md`); its
engine tests (`a_reused_slot_is_fresh_*`) exercise the same behaviour
through a real pooling allocator.
