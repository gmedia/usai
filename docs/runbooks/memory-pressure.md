# Memory pressure / OOM

The runtime's resident set is dominated by the world pool: each of the
`--max-worlds` slots keeps up to `USAI_WASM_KEEP_RESIDENT` (8 MiB) of memory
resident after use, so the plateau is roughly `base + touched slots ×
min(working set, keep_resident)`, where *touched slots* is the peak
concurrency the instance has seen, not the request count. Measured
(`docs/measurements/2026-09-20-p8e-efficiency.md`): base ≈ 40 MiB RSS with a
PostgreSQL pool, a task queue and a cron scheduler; ≈ 4 MiB RSS (≈ 1.5 MiB
PSS — most of the 4 MiB is copy-on-write image pages every slot shares) per
touched slot, so a 48-world instance that has seen c=16 sits at ≈ 100 MiB
and does not come back down while idle. Compiled images add ~30 MiB per
held revision. The P6 soak reports the actual plateau for a workload
(`rssMax` in the soak samples); `/_usai/status` reports `process.rssKib`
and `process.pssKib` live.

Two things sit on top of that plateau and are **not** the world pool:

- **Password hashing.** `crypto.password.hash`/`verify` is Argon2id at
  19 MiB per call, on the blocking pool (up to one per core at once); the
  allocator hands the memory back after each call (`malloc_trim`), but a
  login burst still peaks at ≈ 20 MiB × concurrent hashes above the plateau.
  A drill measured +150–200 MiB retained after a burst on a build before the
  trim; if you see that on a current build, report it.
- **Held revisions.** A replacement holds two compiled images (~30 MiB each)
  until the old one retires; a rollback-and-forth holds them longer. The
  heap is returned when the revision is removed.

## What you see when the limit is too low

`docker inspect` shows `OOMKilled: true`; the log has no error before a
new `wasm engine` line (the kernel killed the process); the proxy answers
502 while it restarts, and with a restart policy this repeats. Measured: a
96 MB limit on a 256-world runtime → restart loop every ~3 s.

## What to do

Size the limit for `40 MiB + max_worlds × 4 MiB` (192 MiB is the supported
floor for 48 worlds, `SUPPORTED.md`), or lower `--max-worlds`. For a box
whose bursts are rare, `USAI_WASM_KEEP_RESIDENT=0` gives the memory back
after each burst (RSS 100 → 45 MiB measured) at the price of re-faulting the
image for every world: ≈2× the CPU per request and half the throughput on
one vCPU (the C2 table in the P8E report; values between 0 and the default
8 MiB change nothing). Raise the limit to ≥ 512 MB for 256 worlds. A
memory-limited instance with fewer worlds refuses with 503
`capacity_exhausted` instead of dying — the right failure. Watch
`usai_worlds_live` and the container's RSS together: RSS that keeps rising
**over hours** while `usai_worlds_live` and the number of held revisions do
not — after a password-hashing burst has had a minute to settle — is a
leak; report it with the soak samples (`usai_process_proportional_memory_bytes`
beside RSS: PSS rising is the honest signal, RSS alone counts shared image
pages).
