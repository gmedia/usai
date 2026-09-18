# vendor/

Third-party sources carried with a local patch. Each entry says what is
patched and why; regenerate with the recipe, do not hand-edit beyond the
listed patch.

## wasmtime 48.0.2 — pagemap slot reset: complete traversal, and reset paged-out dirty pages

`src/runtime/vm/sys/unix/pagemap.rs`, one function, two behaviours
(`wasmtime-pagemap-reset.patch`):

1. **Complete traversal.** The pooling allocator's slot reset scans the
   pagemap for dirty regions into a fixed buffer of 32 regions and treated
   the kernel's `walk_end` as the end of what it would ever reset — a
   QuickJS heap dirties more disjoint runs than that, so the scan stopped
   early and everything past it was `madvise(DONTNEED)`ed: every world
   refaulted its heap (95–315 minor faults per request) and the
   `madvise`/`mprotect` storm capped multi-core scaling with TLB-shootdown
   IPIs (`docs/measurements/2026-09-18-execution-path-attribution.md`).
   The patched function resumes the scan from `walk_end` whenever the
   region buffer filled up, and stops early only when the resident page
   budget (`keep_resident`) is spent — which is what `walk_end` is for.
   Result: the reset touches only dirty pages, 0 faults, hello 2.1 → 1.0 ms
   on the developer machine, ×5 throughput at c=16 on the research VM.

2. **Freshness under memory pressure.** The scan matched dirty pages only
   when they were `PRESENT`. A dirty page the kernel has swapped out is
   `WRITTEN | SWAPPED`, not `PRESENT`, so it was neither reset nor
   decommitted and the next instance of the slot read the previous
   instance's bytes (a QuickJS heap from another world → traps in `dlfree`
   / out-of-bounds accesses under load). Upstream has the same hole for
   every page before its walk end; with 32 regions the accidental decommit
   of everything after it hid most of it. The mask now requires `WRITTEN`
   and not `PFNZERO`/`FILE` regardless of residency; resetting a swapped
   page pages it in and overwrites it.

Regression tests (`engine::wasm::tests`): `a_reused_slot_is_fresh_even_after_its_pages_were_paged_out`
(`MADV_PAGEOUT` between two worlds in one slot — fails with the old mask on
a host with swap, passes with the new one; without swap it only checks the
ordinary reset), `a_reused_slot_is_fresh_after_fragmented_writes` (one byte
every other page: hundreds of regions, more than one scan call returns), and
the same property on the memcpy path without the scan.

Recipe:

```bash
cp -r ~/.cargo/registry/src/index.crates.io-*/wasmtime-48.0.2 vendor/wasmtime
rm -f vendor/wasmtime/.cargo_vcs_info.json vendor/wasmtime/.cargo-ok vendor/wasmtime/Cargo.lock vendor/wasmtime/Cargo.toml.orig
patch -p1 -d vendor/wasmtime < vendor/wasmtime-pagemap-reset.patch
```

`Cargo.toml` `[patch.crates-io]` points `wasmtime` here; every other
`wasmtime-*` crate comes from crates.io at the same version. Upstream: both
behaviours to be proposed (the second is a correctness issue); drop the
vendor when released.
