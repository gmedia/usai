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
(its samples carry all three numbers below); `/_usai/status` reports
`process.rssKib` and `process.pssKib` live, and the cgroup's charge against
its limit when the process runs under one.

**Read the right one.** The three disagree, and on this runtime they
disagree by tens of MiB in both directions at once: `rssKib` counts the
pooled Wasm image once per slot it is mapped into (over), while a cgroup is
not charged for text pages another cgroup faulted in first (under). Measured
side by side on the 24 h bounded soak: **89 MiB RSS, 66 PSS, 53 charged**
(`../measurements/2026-09-24-bounded-soak.md`). Use RSS for *trend*, PSS for
a process-level *level*, and the cgroup's charge for anything about a limit —
it is the number that gets the container killed. `usai top` prints the
charged/limit pair first for exactly that reason, and shouts when the window
had reclaim events, which is how a too-small limit fails *without* an OOM
kill.

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

`docker inspect` shows `OOMKilled: true` — on Kubernetes,
`kubectl describe pod` and `lastState.terminated.reason: OOMKilled` with exit
code **137**. The log has no error before a new `wasm engine` line (the
kernel killed the process); the proxy answers 502 while it restarts, and
with a restart policy this repeats (Kubernetes: `CrashLoopBackOff`, whose
backoff reaches five minutes). Measured: a 96 MB limit on a 256-world
runtime → restart loop every ~3 s.

## What you see when the limit is *almost* too low

There is a state between "fits" and "OOM-killed", and it is worse to
diagnose than either: the container sits **at** its memory ceiling and the
kernel keeps taking back the only thing it can take back — the clean file
pages of the binary the process is executing — which the process then faults
in again. Nothing is killed. No error is logged. Throughput collapses.

Measured on a deliberately undersized box (2026-09-23,
`docs/measurements/2026-09-23-floor-accounting.md`): a `hello`-only instance
in a 48 MiB box answered **1 request per second at a p50 of 4.7 seconds**,
with 1.5 million `memory.events` `max` — and a resident set *smaller* than
the same instance in a 64 MiB box, which served 2 080 req/s at p50 2.1 ms.

How to tell it apart from a slow application:

```bash
# cgroup v2, inside the container or from the host's cgroup path
cat /sys/fs/cgroup/memory.events      # `max` climbing by thousands per second
cat /sys/fs/cgroup/memory.current     # pinned at memory.max
grep -E '^(pgmajfault|pgrefill)' /sys/fs/cgroup/memory.stat   # major faults climbing
```

`usai_process_resident_memory_bytes` going *down* while latency goes up is
the signature. The fix is the limit, not the application: give it the memory
the sizing section asks for.

## What to do

Size the limit for `40 MiB + max_worlds × 4 MiB` (192 MiB is the supported
floor for 48 worlds, `SUPPORTED.md`), or lower `--max-worlds`. For a box
whose bursts are rare, `USAI_WASM_KEEP_RESIDENT=0` gives the memory back
after each burst (RSS 100 → 45 MiB measured) at the price of re-faulting the
image for every world: ≈2× the CPU per request and half the throughput on
one vCPU (the C2 table in the P8E report; values between 0 and the default
8 MiB change nothing). The same formula at the default `--max-worlds` of 256 asks for ≈ 1 GiB, which is the honest number for a box that really admits 256 concurrent worlds — a smaller box lowers `--max-worlds` rather than the limit. A
memory-limited instance with fewer worlds refuses with 503
`capacity_exhausted` instead of dying — the right failure. Watch
`usai_worlds_live` and the container's RSS together: RSS that keeps rising
**over hours** while `usai_worlds_live` and the number of held revisions do
not — after a password-hashing burst has had a minute to settle — is a
leak; report it with the soak samples (`usai_process_proportional_memory_bytes`
beside RSS: PSS rising is the honest signal, RSS alone counts shared image
pages).
