# Sizing an instance: worlds, pool, concurrency, memory, replicas

Four numbers decide what one instance can do, and they are nested. Set them
in this order; the measurements they come from are in
`docs/measurements/2026-09-20-p8e-efficiency.md` (P8E) and
`2026-09-18-p5-p6-qualification.md` (P5/P6).

```text
--max-worlds  ≥  Σ per-workload concurrency you want at once  ≥  pool.max (per database)
memory limit  ≈  40 MiB + max_worlds × 4 MiB (+ 30 MiB per held revision)
```

## 1. `--max-worlds` — how much work at once

One world per unit of work in flight: a request being served, a task
running, a cron tick, a queue message, a held WebSocket or event stream.
`--max-worlds` (`USAI_MAX_WORLDS`, default 256) is the admission bound: the
next request past it is `503 capacity_exhausted` before a world exists — a
verdict, not a failure (`overload.md`). Size it from the peak concurrency you
want to *serve*, not from throughput: at 1 000 req/s with a 15 ms p50 you
hold ≈15 worlds; at 5 ms, ≈5. Add the connection-bound worlds you hold
(every open WebSocket and stream is one world for its whole life, until
`USAI_SOCKET_IDLE_TIMEOUT` for a vanished client) and the background ones
(queue consumers × `concurrency`, running tasks, a cron tick).

Measured: 48 worlds served a 16-client burst of a PostgreSQL read at
1 015 req/s on one vCPU with 0 refusals (P8E §3); 8 worlds against the same
16 clients refused ≈half (admission working as designed); the production
compose runs 48.

## 2. `pool.max` — how many worlds may hold a connection

`postgres("db", { pool: { max } })` (default 16) bounds the connections an
instance opens. A world leases one per operation and gives it back; a
transaction pins one for its duration. When every connection is out, the
next lease waits, then the world is `503 resource_exhausted`. Rules:

- `pool.max` ≤ `--max-worlds`: more connections than worlds can never be used.
- `pool.max` ≈ the number of worlds that are *in a query at the same moment*,
  not the number of worlds: a request that spends 2 ms of its 15 ms in
  PostgreSQL holds a connection for 2 ms. P8E ran 48 worlds on a pool of 4
  at 1 015 req/s (`pool.max` 4 is the template's choice for density, not a
  recommendation for a busy service; 16 is the default for one).
- **Across replicas the PostgreSQL side adds up**: N instances × `pool.max`
  connections against `max_connections` (100 by default on PostgreSQL). Fifty
  small instances at `pool.max` 4 are 200 connections; ten at 16 are 160.
  Size `max_connections` (or a pooler in front) from that product.
- Long transactions (`sql.transaction`) hold a connection for the whole
  callback: keep them short, or give them their own pool via a second
  `postgres()` declaration on the same URL.

## 3. Per-workload `concurrency`

`concurrency` on a task, cron, or queue consumer bounds *that workload's*
worlds: `queue.consume("x", { concurrency: 8 })` is at most eight messages in
flight on this instance; a task's `concurrency` refuses dispatches past it
with `capacity_exhausted`. It never raises `--max-worlds`; it partitions it.
Start from the dependency the workload talks to: a consumer doing one query
per message wants `concurrency` ≤ the share of `pool.max` you give it.

## 4. Memory

Measured plateau (P8E §4): **≈40 MiB base** (PostgreSQL pool, task queue,
cron scheduler, status listener up) **+ ≈4 MiB RSS per world slot ever
touched** (≈1.5 MiB of it unique, PSS; the rest is the image's shared
pages), reached at the peak concurrency seen, not growing with request
count, and **not returning while idle** (the slot keeps what it touched so
the next world does not fault it back in; `USAI_WASM_KEEP_RESIDENT=0` gives
it back at half the throughput). Each held revision adds ≈30 MiB of compiled
image (one during a replacement, bounded).

```text
mem_limit / MemoryMax  ≥  40 + peak_concurrency × (4…8) + 30   (MiB)
48 worlds → ≥ 262 MiB; the compose file uses 512 MiB; 192 MiB is the supported floor, re-measured 2026-09-23 on a box charged for its own page cache (128 MiB serves the same shape with zero reclaim but leaves nothing for a held revision — `docs/measurements/2026-09-23-floor-accounting.md`)
```

The runtime checks its own cgroup at start, but against a **floor**, not
against this arithmetic *(since 0.0.10; **0.0.9 checks the full arithmetic
instead** and therefore warns at the 192 MiB envelope this page and
`SUPPORTED.md` recommend — if you run 0.0.9, that warning is a known false
positive, not a sizing problem)*: `40 + 30 + max_worlds × 1` MiB, the point below
which the box cannot hold the runtime, one held revision and the *unique*
memory of its slots at all (48 worlds → 118 MiB). Below that it warns —
`the memory limit is below the floor for this --max-worlds`, with both
numbers. It deliberately stays quiet at the envelope `SUPPORTED.md` tells you
to deploy on: a warning that fires on every correct deployment is one
operators learn to ignore, and the failure it guards against is the one an
orchestrator cannot see. Sizing above the floor is the arithmetic above, and
the signal that you got it wrong is
`usai_process_memory_ceiling_hits_total` climbing.

**The limit also has to cover the binary the container reads.** A cgroup is
charged for the page cache the container faults in, and that includes the
runtime's own text — ~35 MB of `usai` on disk, of which the working set is
tens of MiB. It is already inside the measured plateau above, but it is the
reason a limit that looks generous against RSS can still be too small: when
the charge reaches the ceiling the kernel reclaims those pages and the
process faults them straight back in, without an OOM kill and without a log
line. Watch `usai_process_memory_ceiling_hits_total` after a sizing change;
`memory-pressure.md` has the symptom and the measurement.

**Per-slot is 4 MiB for a small application and up to 8 for a real one.**
P8E measured ≈4 MiB RSS per touched slot on the hello application; the
concurrency sweep's application — PostgreSQL, contracts on six routes,
auth — showed ≈8 MiB per concurrent request (≈0.5 GiB resident at c=64,
`2026-09-23-sweep.md`). Size from the peak concurrency you intend to serve,
with your own application: `usai run --max-worlds <n>`, drive it at that
concurrency, and read `usai_process_resident_memory_bytes` at the plateau.
The number does not grow with request count, and it does not come back
while the process lives (`USAI_WASM_KEEP_RESIDENT`).

**Per-workload memory** on top of that formula, for the operations that
allocate more than a world does:

| Operation | Peak per concurrent call | Retained after |
|---|---|---|
| `crypto.password.hash` / `verify` (Argon2id, 19 MiB, 2 passes) | ≈ 20 MiB, on the blocking pool (≤ one per core at once) | none — the heap is trimmed after each call; budget `cores × 20 MiB` for a login burst |
| A body up to `USAI_MAX_BODY_BYTES` (1 MiB default) | ≈ 3× the body (bytes, the guest copy, the decoded value) | none |
| A held revision (replacement, rollback) | ≈ 30 MiB compiled image | until it is removed |
| A `cache.local` | its declared `maxEntries × entry size` | for the revision's life |

Go below that and the box fails at the first burst, not at idle — and it may
fail **without being OOM-killed**, which is the failure that wastes a night.
Re-measured on a box charged for its own page cache
(`docs/measurements/2026-09-23-floor-accounting.md`): at 48 MiB the hello
application is not killed, it *thrashes* — charged to its ceiling, reclaiming
its own executable text 1.4 M times, serving **1 req/s**. The technical floor
is **64 MiB**, and 48 MiB is not a small box, it is a broken one. Read a floor
from resident set and reclaim events (`memory.events`), never from the absence
of an OOM kill.

Watch `usai_process_resident_memory_bytes` against `usai_worlds_live`: RSS
climbing while worlds do not is a leak. Size from **PSS**, not RSS: the
pooled Wasm image is one set of physical pages mapped into every slot, and
RSS counts it once per slot — measured on the 24 h bounded soak, a process
at 89 MiB RSS was 66 MiB PSS and its cgroup was charged 53
(`docs/measurements/2026-09-24-bounded-soak.md`). The number that gets you
OOM-killed is the cgroup's, which `/_usai/status` reports beside the limit
when the process runs under one.

## 5. CPU

One vCPU serves ≈1 000 PostgreSQL-read requests per second at c=16 (p50
15 ms, the pool of 4 being the limiter) or ≈2 200 hello requests at c=4
(P8E §2–3); the runtime is multi-threaded, so more cores are more worlds in
parallel without a cluster. A **fractional vCPU** works — the floors ran at
0.25 — but its burst p99 is the CFS throttle (60–80 ms at 0.25–0.5 vCPU),
so give a latency-sensitive service a whole one. Idle costs 0.00 % (the
watchdog and epoch tickers park while no world is live).

## 6. Replicas

Two replicas behind a proxy with an active health check on `/_usai/ready`
and a retry of refused connections give zero-502 rolling restarts
(`deploy-and-rollback.md`); a single replica costs 2–3 s of 502 per restart.
Cron: `exclusive: true` on the schedule, or `--no-cron` on all but one.
Queues share work with no configuration; services run on every instance
that runs them (`--no-services` elsewhere). Fifty mostly-idle instances cost
≈30 MiB PSS each and no idle CPU (P8E §5) — one process per application is
cheap; the per-application intercept is the open question (Q17).

## Worked example

An internal API, PostgreSQL-backed, 200 req/s peak with a 10 ms p50, a
queue consumer at 4, two replicas:

- in flight ≈ 200 × 0.010 = 2 request worlds; + 4 consumer worlds + a tick →
  `--max-worlds 16` is generous (leave the default 256 only on a box with
  the memory for it).
- connections: 2 replicas × `pool.max` 8 = 16 on PostgreSQL.
- memory: 40 + 16 × 4 + 30 = 134 MiB → `MemoryMax=256M` with headroom.
- CPU: one vCPU each; `--drain-grace 2`, `TimeoutStopSec=40`.
