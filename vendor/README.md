# vendor/

Third-party sources carried with a local patch. Each entry says what is
patched and why; regenerate with the recipe, do not hand-edit beyond the
listed patch.

## wasmtime 48.0.2 — `MAX_REGIONS` 32 → 1024

`src/runtime/vm/sys/unix/pagemap.rs`: the pooling allocator's slot reset scans
the pagemap for dirty regions into a fixed buffer of 32 regions; a QuickJS
heap dirties more disjoint runs than that, so the scan stopped early and
everything past it was `madvise(DONTNEED)`ed — every world refaulted its heap
(95–315 minor faults per request) and the `madvise`/`mprotect` storm capped
multi-core scaling with TLB-shootdown IPIs (`docs/measurements/2026-09-18-execution-path-attribution.md`).
With 1024 regions the reset touches only dirty pages: 0 faults, hello world
2.1 → 1.0 ms on the developer machine.

Recipe:

```bash
cp -r ~/.cargo/registry/src/index.crates.io-*/wasmtime-48.0.2 vendor/wasmtime
rm -f vendor/wasmtime/.cargo_vcs_info.json vendor/wasmtime/.cargo-ok
patch -p1 -d vendor/wasmtime < vendor/wasmtime-max-regions.patch
```

`Cargo.toml` `[patch.crates-io]` points `wasmtime` here; every other
`wasmtime-*` crate comes from crates.io at the same version. Upstream: make
the region budget configurable (to be proposed); drop the vendor when released.
