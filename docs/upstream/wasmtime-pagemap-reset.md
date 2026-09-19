# Upstreaming the Wasmtime pagemap reset patch

Status (2026-09-19): **PR open** —
<https://github.com/bytecodealliance/wasmtime/pull/14357> (fork
`HasanH47/wasmtime`, branch `pagemap-reset-complete-traversal`: the patch
plus three unit tests in `pagemap.rs`, on upstream `main` at `7ad2e73`).
Review feedback is applied on that branch; the vendored patch follows what
lands. Upstream `main` still has both behaviours this repository patches (`crates/wasmtime/src/runtime/vm/sys/unix/pagemap.rs`:
`MAX_REGIONS = 32` with `walk_end` treated as the end, and a category mask
requiring `PRESENT`). Latest release: v48.0.2, the version vendored here.
Nothing to rebase yet; the patch applies to `main` as is.

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
