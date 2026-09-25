# Measuring `Engine::release_idle_pool_memory`

The before/after the idle-decommit proposal promised
(`../wasmtime-idle-decommit.md`), as a standalone program rather than
through this runtime, so a Wasmtime maintainer can re-run it without
building Usai.

It warms N pooling slots by instantiating a module and dirtying 4 MiB of
each, drops them all, and samples RSS and `unused_memory_bytes_resident`
around the call. `RELEASE=0` is the control: idle for the same time and do
not call.

```bash
# point Cargo.toml's path dependency at a wasmtime checkout carrying the PR
cargo build --release
RELEASE=0 ./target/release/idlemeasure 64    # control
./target/release/idlemeasure 64              # with the call
```

## Measured 2026-09-25 (Linux 6.18, release build, 64 slots, keep_resident 8 MiB)

| after 5 s idle | no call | `release_idle_pool_memory()` |
|---|---|---|
| `unused_memory_bytes_resident` | 256.0 MiB | 0.0 MiB |
| process RSS | 266.9 MiB | 10.9 MiB |
| next instantiate + 4 MiB touch | 0.18 ms | 2.23 ms |
| the one after that | 0.02 ms | 0.02 ms |

The RSS drop (256.1 MiB) matches the bytes the call reports (256.0 MiB), so
the counter is not overstating what it gave back, and the control shows the
memory does not come back on its own.

**The last two rows are the cost side**, measured rather than asserted: the
first instantiation after the quiet period pays ~2 ms re-faulting its pages,
and the slot is warm again immediately after. That is what an embedder
accepts when it decides it is idle.
