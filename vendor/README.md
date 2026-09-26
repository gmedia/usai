# vendor/

Third-party sources carried with local patches. Each entry says what is
patched and why; regenerate with the recipe, do not hand-edit beyond the
listed patches. **Both patches are merged upstream**, so this directory is
waiting on releases, not on review.

## wasmtime 48.0.2 — pagemap slot reset: complete traversal, and reset paged-out dirty pages

Upstream: <https://github.com/bytecodealliance/wasmtime/pull/14357>
**merged into `main` on 2026-09-23** (`a2e2d86`;
`docs/upstream/wasmtime-pagemap-reset.md`). `src/runtime/vm/sys/unix/pagemap.rs`
here is byte-identical to upstream `main` (checked 2026-09-23; upstream also
carries the three unit tests, which this vendored 48.0.2 copy does not).
v49.0.0 was released two days before the merge, so no release carried it
yet.

> **Do not go by the release date.** This file used to say the directory
> goes away with "the first wasmtime release after 2026-09-23", and that is
> wrong: **v49.0.1 (checked 2026-09-25) does not carry the patch** — its
> `pagemap.rs` is byte-identical to v49.0.0's, because a patch release did
> not touch this file. Following the date rule would have deleted the vendor
> and silently returned the runtime to 95–315 minor faults per request.
>
> Go by the **file**. Before dropping this directory, check that the release
> you are moving to actually contains the change:
>
> ```bash
> ref=v49.1.0   # whichever release you are considering
> gh api "repos/bytecodealliance/wasmtime/contents/crates/wasmtime/src/runtime/vm/sys/unix/pagemap.rs?ref=$ref" \
>   -q .content | base64 -d > /tmp/upstream-pagemap.rs
> # the production half must match what is vendored here
> t=$(grep -n 'mod tests' vendor/wasmtime/src/runtime/vm/sys/unix/pagemap.rs | head -1 | cut -d: -f1)
> u=$(grep -n 'mod tests' /tmp/upstream-pagemap.rs | head -1 | cut -d: -f1)
> diff <(head -n $((t-1)) vendor/wasmtime/src/runtime/vm/sys/unix/pagemap.rs) \
>      <(head -n $((u-1)) /tmp/upstream-pagemap.rs) && echo "the release carries it"
> ```

When a release does carry it: re-vendor nothing, delete `vendor/`, drop the
`[patch.crates-io]` entry, bump the `wasmtime` dependency, and run the
freshness tests — they are the check that the released crate really has both
behaviours.

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

## wasmtime 48.0.2 — `Engine::release_idle_pool_memory`

Upstream: <https://github.com/bytecodealliance/wasmtime/pull/14419>
**merged into `main` on 2026-09-26** (`65c5190`, approved by a Wasmtime
maintainer; `docs/upstream/wasmtime-idle-decommit.md`). The patch here is that
PR's merged diff with the `crates/wasmtime/` prefix stripped, applied
unchanged — it lands cleanly on 48.0.2, which is why the lever did not have to
wait for a release.

Adds one public method and the two internal pieces it needs: an index
allocator that can take warm-but-unused slots out of the free lists with their
affinity intact, and a memory pool that decommits their resident regions and
puts the slots back. Nothing is lost by releasing: on a platform whose
`decommit_behavior` is `RestoreOriginalMapping` the resident region holds
exactly what the mapping restores on the next touch.

Why this repository needs it: `linear_memory_keep_resident` is fixed when the
`Engine` is built, so resident memory follows the peak concurrency the process
has ever seen rather than its current load — 64 concurrent worlds cost half a
gigabyte and keep it all night. ADR-0019 lever 2; the call site is the epoch
ticker in `engine/wasm.rs`, which is the one thread that knows the process has
gone quiet.

Regression test: `engine::wasm::tests::the_idle_release_respects_its_floor_and_leaves_the_slot_usable`.
Unlike the pagemap patch there is no freshness property to re-check — if the
method disappears, the build fails.

Recipe:

```bash
cp -r ~/.cargo/registry/src/index.crates.io-*/wasmtime-48.0.2 vendor/wasmtime
rm -f vendor/wasmtime/.cargo_vcs_info.json vendor/wasmtime/.cargo-ok vendor/wasmtime/Cargo.lock vendor/wasmtime/Cargo.toml.orig
patch -p1 -d vendor/wasmtime < vendor/wasmtime-pagemap-reset.patch
patch -p1 -d vendor/wasmtime < vendor/wasmtime-idle-decommit.patch
```

`Cargo.toml` `[patch.crates-io]` points `wasmtime` here; every other
`wasmtime-*` crate comes from crates.io at the same version.

**Dropping the vendor now takes two checks, not one.** Both patches are
merged, but they will not necessarily be in the same release, and the rule
above still holds: go by the file. `pagemap.rs` as shown earlier, and for this
one:

```bash
ref=v50.0.0   # whichever release you are considering
gh api "repos/bytecodealliance/wasmtime/contents/crates/wasmtime/src/engine.rs?ref=$ref" \
  -q .content | base64 -d | grep -c release_idle_pool_memory
```

A release carrying one and not the other means the vendor stays with the patch
that is still missing — and the `[patch.crates-io]` entry with it.
