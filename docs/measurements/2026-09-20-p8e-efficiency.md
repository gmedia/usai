# P8E — the efficiency envelope: floors, residency, density (2026-09-19/20)

> **Correction, 2026-09-23 — both floors in this report are under
> re-measurement and should not be quoted until it lands.** Every boxed cell
> here bind-mounted the `usai` binary and the artifact into the container.
> Page cache is charged to the cgroup that *first* faults a page in, and the
> host had already run the same binary, so the box was not billed for the
> runtime's text: the cell passed partly on memory somebody else paid for.
> The same method was used for the comparator on 2026-09-23 and produced an
> impossibility — 88 MiB of resident memory in a 48 MiB box, no OOM kill —
> which is what exposed it (`docs/measurements/2026-09-23-floor-accounting.md`).
> The harness now drops the page cache for what a cell is about to run, so
> the box faults its own pages and is charged for them, and records what the
> kernel charged it. **Density
> (§5) and the residency finding (§4) are unaffected**: they are bare
> processes on the host, not boxed cells.

P8 (`2026-09-19-p8-parity.md`) answered what one request costs. This
campaign answers the other axis, which nobody had measured: what an
application costs when it does mostly nothing, how small a host runs it
correctly, and how RAM and CPU grow from one to fifty mostly-idle
applications on a shared host. The lifecycle model predicts an advantage
here (a persistent runtime with no persistent mutable application state
should be cheap when idle; worlds appear only when work does). That was a
hypothesis. Below is what came out, including the part that did not go the
model's way.

Three questions, three answers:

| Question | Answer | Cell |
|---|---|---|
| **Technical floor** — one application, hello only, no PostgreSQL | **48 MiB, 0.25 vCPU** (32 MiB is OOM-killed on the first request) | `hello-48m-0.25c-{1,16}w`: 0 errors at c=1 and c=4, peak RSS 45–46 MiB, ≈2 MiB of headroom |
| **Supported floor** — the production shape (PostgreSQL pool 4, task, cron, status + metrics, JSON logs) | **128 MiB, 1 vCPU, 48 worlds passes** (peak 102 MiB, 20 % headroom); **192 MiB is the number written into `SUPPORTED.md`** (≥ 25 % headroom) | `template-{128,192}m-1c-48w`: 1 015 req/s at c=16, p50 15 ms, p99 24 ms, 0 errors, no OOM in any of 30 cells |
| **Density** — N mostly-idle applications per host | **≈30 MiB PSS and 0.00 % CPU per idle Usai application at N=50**, against ≈36 MiB and 0.12 % for Node + Fastify serving the same routes: 17 % less proportional memory, 44 % less RSS, an order of magnitude less idle CPU, 4× faster cold first response — and ≈1.7× more CPU per request during the burst | `density-{usai,node}-{1,5,10,25,50}` |

And one finding against the prediction: **memory does not return after a
burst** — the world pool keeps what it touched (§4).

Engineering numbers on one machine with a disclosed co-tenant, not
canonical evidence (`BENCHMARKS.md` → Disclosure).

## 1. Conditions

- **Host**: VM 47 — 16 × Xeon E5-2680 v4, 32 GiB, Ubuntu 26.04 LTS, kernel
  7.0.0-31, Docker 29.7.2. The fleet ran on **cpus 12–15** (`taskset` for
  bare processes, `--cpuset-cpus` for containers). **Co-tenant**: the P6
  24 h soak on cpus 0–11 (Caddy → runtime → PostgreSQL, ≈1 000 req/s) ran
  throughout; it competes for memory bandwidth and the kernel, not for the
  fleet's cores. The envelope is about floors and idle behaviour, not
  latency; latency figures here are indicative only.
- **Subject**: commit `74a5faa` (fleet run, 2026-09-19 09:41 → 18:55 UTC),
  release build, Wasm engine (ADR-0016) with the P8 levers. The C2 cells
  (§4) ran on `615dc4e` (2026-09-20 00:29 UTC).
- **Applications**: `examples/hello` for the technical floor;
  `scripts/qualification/p8e/template/` for everything else — the bench
  app's shape (`GET /hello/:name`, `GET /users/:id` one pooled read,
  `POST /users`, `GET /health`) plus a task and a cron so the task queue and
  the scheduler exist, `pool.max` 4. Artifacts precompiled; no compile
  cache in the boxes.
- **Boxes** (floors): plain `ubuntu:26.04` container, the binary and the
  artifact bind-mounted read-only, `--read-only --tmpfs /tmp`, run as the
  invoking user, `--memory X --memory-swap X` (a breach is a real OOM kill,
  never swap), `--cpus`, host network. Status listener on, metrics scraped
  by the sampler every second, JSON logs.
- **Comparator** (density): Node v24.20.0 + Fastify 5.12.5 + `pg` pool 4 +
  Zod on input and output (`scripts/qualification/bench/baselines/node-fastify`),
  the same routes, same PostgreSQL.
- **PostgreSQL**: 18.6 in a container on the host, one throwaway database
  shared by every tenant.
- **Sampler**: `scripts/qualification/p8e/sample.sh`, once a second per
  process: RSS and PSS (`smaps_rollup`), VmSize, faults, CPU ticks (user
  and system separately — memory-cgroup reclaim is charged to the process
  as system time), threads, fds; host `MemAvailable` and context switches.
- **Load**: the suite's closed-loop client (`load.mjs`) for the floors;
  `oha -q 100` for the density bursts; `curl` for the trickle.
- **Driver and tables**: `scripts/qualification/p8e/fleet.sh`,
  `report.mjs`. Raw run directory on the VM:
  `~/bench-out/p8e-20260919T094129Z-74a5faa` (72 cells, samples, phases,
  load files, status and metrics dumps, container logs).

**RSS vs PSS.** Both are reported; PSS is the honest one for this runtime.
Every world slot maps the same copy-on-write image pages; RSS counts a
shared page once per mapping, PSS divides it among its sharers. The gap
grows with the number of slots that have been touched (§4) and with the
number of processes on the host (§5): at N=50 the fleet's RSS sum
overstates its footprint by 58 %.

**What a cgroup limit counts.** The 32 MiB hello boxes show RSS 39 MiB
*inside* a 32 MiB cgroup and are OOM-killed only at the first request. The
kernel charges a file-backed page to whoever first faulted it; the artifact
and the binary were already in the host's page cache, so ≈16 MiB of the
idle RSS is not charged to the box (`dmesg`: `anon-rss:26556kB
file-rss:16500kB`). A memory limit is spent on **anonymous memory plus
pages the box faults in itself**; idle RSS is not the number to size
against. Peak RSS under load, from the same table, is.

## 2. Technical floor — `examples/hello`, no PostgreSQL

Matrix: memory 32 / 48 / 64 / 96 / 128 MiB × cpus 0.25 / 0.5 / 1 ×
`--max-worlds` 1 / 16 = 30 cells. Pattern: idle 60 s → c=1 60 s → c=4 60 s →
idle 60 s. Pass: ready, 0 errors, no OOM, still ready at the end.

| memory | cpus | worlds | ready s | idle RSS/PSS MiB | c=1 p50/p99 ms | c=4 p50/p99 ms | peak RSS | after idle | verdict |
|---|---|---|---|---|---|---|---|---|---|
| **32** | any | any | 0.56–1.10 | 39 / 34 | — | — | — | — | **OOM-killed at the first request** (6/6 cells; `dmesg`: memcg, `anon-rss 26.5 MiB + file-rss 16.5 MiB`) |
| **48** | 0.25 | 1 / 16 | 1.11 / 1.09 | 39 / 34 | 0.76 / 63.5 | 2.11 / 79.3 | 45–46 | 45–46 | pass |
| 48 | 0.5 | 1 / 16 | 0.58 / 0.56 | 39 / 34 | 0.70 / 8.2 | 2.09 / 53.7 | 45 | 45 | pass |
| 48 | 1 | 1 / 16 | 0.58 / 0.57 | 39 / 35 | 0.68 / 1.16 | 2.10 / 3.58 | 45–46 | 45–46 | pass |
| 64 | 0.25 / 0.5 / 1 | 1 / 16 | 0.56–1.11 | 39 / 34 | 0.68–0.76 | 2.08–2.12 | 45–46 | 45–46 | pass (identical to 48) |
| 96 | 0.25 / 0.5 / 1 | 1 / 16 | 0.56–1.14 | 39 / 34 | 0.67–0.75 | 2.08–2.12 | 45–46 | 45–46 | pass |
| 128 | 0.25 / 0.5 / 1 | 1 / 16 | 0.56–1.12 | 39 / 35 | 0.67–0.75 | 2.07–2.12 | 45–46 | 45–46 | pass |

Throughput at c=4: 510 req/s at 0.25 cpu, 1 076 at 0.5, 2 222 at 1 cpu —
CPU-bound at every fraction, as a runtime tax microscope should be. The
p99 at 0.25 and 0.5 cpu (63 / 79 ms, 8–9 / 54 ms) is **CFS bandwidth
throttling**: a quarter of a core is 25 ms of run time per 100 ms period,
and a request that arrives when the quota is spent waits for the next
period. It is the cost of a fractional vCPU, not of the runtime; at 1 cpu
p99 is 1.2 / 3.5 ms.

Idle: **0.00 % CPU, 0.00 % system time**, 3–4 threads (`available_parallelism` honours the cgroup quota), 12–13 fds, in every
cell. That is `crate::idle` (§6): before it, an idle runtime woke 300 times
a second for the watchdog and the epoch ticker.

**Verdict (void — see the correction at the top).** The ≈2 MiB of headroom
below is the clearest casualty of the accounting error: re-measured on a box
that pays for its own page cache, a 48 MiB `hello` cell is never OOM-killed
and serves **one request per second**, thrashing on its own text
(`2026-09-23-floor-accounting.md`). What follows is kept as the record of
what was measured and how.

The technical floor is **48 MiB and 0.25 vCPU**, with ≈2 MiB of
headroom at the peak — a number to quote with its condition (hello only,
c ≤ 4), not to deploy on. Memory usage is the same from 48 to 128 MiB and
from 1 to 16 worlds; the pool's slot reservation is virtual (VmSize is
large by design), and only touched slots cost anything (§4).

## 3. Supported floor — the template with PostgreSQL

*(Void — see the correction at the top. The re-measured template cells, on a
box charged for its own page cache, hold their peak at 96–106 MiB whatever
the box size and never touch their ceiling; the number that changes is what
the claim rests on, not the peak.
`docs/measurements/2026-09-23-floor-accounting.md`.)*

Matrix: memory 128 / 192 / 256 / 384 / 512 MiB × cpus 0.5 / 1 / 2 ×
`--max-worlds` 8 / 48 = 30 cells. Pattern (the production shape): idle
120 s → trickle 1 req/s for 180 s alternating hello and a PostgreSQL read →
burst c=16 for 60 s on `GET /users/:id` (class C) → idle 300 s. The 8-world
cells take c=16 on purpose: they show what admission does when the offered
load exceeds the pool.

| memory | cpus | worlds | ready s | idle RSS/PSS | burst req/s | burst p50/p99 ms | 503 `capacity_exhausted` | 5xx | peak RSS | after 300 s idle | OOM |
|---|---|---|---|---|---|---|---|---|---|---|---|
| 128 | 0.5 | 48 | 0.57 | 40 / 35 | ≈900 | 15.9 / 71.5 | 0 | 0 | 100 | 100 | no |
| **128** | **1** | **48** | 0.57 | 40 / 35 | **1 015** | **14.8 / 22.8** | 0 | 0 | **100** | 100 | no |
| 128 | 2 | 48 | 0.58 | 40 / 35 | ≈1 900 | 8.3 / 16.7 | 0 | 0 | 102 | 102 | no |
| 128 | 1 | 8 | 0.57 | 40 / 35 | ≈890 admitted | 2.9 / 16.7 (admitted) | 141 004 | 0 | 72 | 72 | no |
| 192 | 1 | 48 | 0.57 | 40 / 35 | ≈1 000 | 15.1 / 23.9 | 0 | 0 | 101 | 101 | no |
| 256 | 1 | 48 | 0.57 | 40 / 36 | ≈1 000 | 15.2 / 24.4 | 0 | 0 | 101 | 101 | no |
| 384 | 1 | 48 | 0.57 | 40 / 36 | ≈1 000 | 15.3 / 29.1 | 0 | 0 | 101 | 101 | no |
| 512 | 1 | 48 | 0.57 | 40 / 35 | ≈1 000 | 15.4 / 29.0 | 0 | 0 | 101 | 101 | no |
| any | 0.5 | 8 | 0.57 | 40 / 35 | admitted | 3.2–3.7 / 63–65 | 59 714–68 979 | 0 | 72–73 | 72–73 | no |
| any | 2 | 8 | 0.57 | 40 / 36 | admitted | 2.5–2.9 / 14–22 | 138 521–158 652 | 0 | 72–73 | 72–73 | no |

Every cell became ready in 0.57 s, idled at **40 MiB RSS / 35 MiB PSS and
0.00 % CPU** with the pool, the scheduler and the task queue up, and was
still ready at the end. **No cell was OOM-killed**, including 128 MiB with
48 worlds. The memory limit did not change a single number across the
column: 128 MiB behaves exactly like 512 MiB. The pool of 4 PostgreSQL
connections is the reason c=16 sits at p50 15 ms: 16 worlds share 4
connections, and PostgreSQL is on the same host.

**Admission, not failure.** In every 8-world cell the c=16 burst produced
60 000–160 000 responses `503 capacity_exhausted` (`rejections.capacity` in
the status dump) and **zero 5xx**: sixteen closed-loop clients against
eight slots means eight are refused before a world exists (C6, ADR-0012),
the admitted requests ran at p50 2.5–3.7 ms, and the process never grew
past 73 MiB. That is the designed answer to more load than the operator
sized for. The first tabulation of the run marked these cells `FAIL` because it
counted 503s as errors; the criterion was wrong, not the cells —
the pass rule is *no 5xx, no OOM, ready at the end*, and a 503 is a
verdict, not a defect.

Fractional CPUs again show CFS throttling (p99 63–73 ms at 0.5 cpu against
23 ms at 1 cpu, same p50 shape); a supported floor needs a whole vCPU for
its burst p99, or a p99 SLO that includes the throttle.

**Verdict.** The smallest passing cell with ≥ 25 % headroom at the burst
peak (102 MiB): **192 MiB**. 128 MiB passes with 20 % (and the C2 knob below
brings its peak down). **Supported floor: 192 MiB, 1 vCPU, `--max-worlds`
48, PostgreSQL `pool.max` 4, status + metrics on** — written into
`SUPPORTED.md`; the production compose's `mem_limit: 512m` stays as the
comfortable default and is now justified by a measured plateau instead of a
formula.

## 4. Memory does not return after a burst (C2)

The plan's pass rule for the supported floor also asked that RSS after the
post-burst idle be within +10 % of the pre-burst idle — "memory returns".
**It does not**, in any cell:

| worlds touched | idle RSS / PSS | after burst RSS / PSS | +300 s idle | per touched slot |
|---|---|---|---|---|
| 8 (`8w` cells at c=16) | 40 / 35 | 72–73 / 49 | unchanged | +4.0 MiB RSS, **+1.75 MiB PSS** |
| 16 (`48w` cells at c=16) | 40 / 35 | 100–102 / 55 | unchanged | +3.8 MiB RSS, **+1.25 MiB PSS** |

The mechanism is the pooling allocator's residency policy, chosen in P2 for
throughput: after a world ends, its slot keeps up to
`linear_memory_keep_resident` (8 MiB, `USAI_WASM_KEEP_RESIDENT`) of the
memory it touched mapped and resident instead of decommitting it, so the
next world in that slot does not fault its working set back in. What stays
is ≈4 MiB per slot for this application, and most of it is the image's
pages the world *read* (bytecode, the SDK's objects) — shared, file-backed,
which is why RSS grows three times faster than PSS. The plateau is bounded
— base + `max_worlds × working set` — and it is reached by the peak
concurrency ever seen, not by request count (the hello cells grew 39 → 45
at c=4 and stopped).

This is the right behaviour for a busy server and the wrong one for a small
box that bursts once an hour, so the knob was measured both ways (same
pattern, commit `615dc4e`, 2026-09-20 00:29–01:26 UTC, the 72 h soak as
co-tenant):

| cell | `USAI_WASM_KEEP_RESIDENT` | idle RSS / PSS | after burst → after 300 s idle | burst req/s | p50 / p99 ms | CPU (sys) during burst | minor faults/s |
|---|---|---|---|---|---|---|---|
| template 256m-1c-48w | 8 MiB (default; 2026-09-19 run) | 40 / 36 | 101 / 55 → **101 / 55** | 1 000 | 15.2 / 24.4 | — | — |
| template 256m-1c-48w | **2 MiB** | 40 / 36 | 101 / 54 → **101 / 54** | 919 | 17.0 / 27.8 | 87 % (16.5 %) | 62 |
| template 256m-1c-48w | **0** | 40 / 36 | 100 / 53 → **45 / 41** | **450** | **35.1 / 51.3** | 92 % (**51.8 %**) | **99 583** |
| template 256m-1c-8w | 2 MiB | 40 / 36 | 72 / 48 → 72 / 48 | 2 727 (incl. 115 731 × 503) | 3.65 / 22.2 (admitted) | 83 % (18 %) | 1 |
| template 256m-1c-8w | 0 | 40 / 36 | 72 / 48 → **45 / 40** | 1 462 (incl. 64 702 × 503) | 7.22 / 36.9 (admitted) | 86 % (47 %) | 85 112 |
| hello 64m-1c-16w | 8 MiB | 39 / 34 | 45 / 41 → 45 / 41 | c=1 1 168 · c=4 1 800 | 0.79 / 1.50 · 2.14 / 5.12 | 62 % (13 %) · 82 % (17 %) | 0 |
| hello 64m-1c-16w | 2 MiB | 39 / 35 | 46 / 41 → 46 / 41 | c=1 1 183 · c=4 1 778 | 0.78 / 1.46 · 2.11 / 4.99 | 62 % (13 %) · 81 % (17 %) | 0 |
| hello 64m-1c-16w | 0 | 39 / 34 | 45 / 41 → 42 / 38 | c=1 **561** · c=4 **698** | **1.66 / 3.00 · 6.59 / 10.2** | 79 % (**47 %**) · 88 % (**53 %**) | 98 142 · 122 031 |

Two facts fall out. **2 MiB is identical to 8 MiB** in every column: what
a world of these applications keeps resident is under 2 MiB of the linear
memory the budget governs, so lowering the budget frees nothing. **0 gives
the memory back** (RSS 100 → 45 MiB, PSS 53 → 41 on the 48-world cell) and
**halves the runtime**: every world faults its working set back in — ≈85–100
minor faults per request, system time from 13–17 % to 47–53 % of the core,
hello p50 0.79 → 1.66 ms, class C p50 15 → 35 ms, throughput −52 to −61 %.
That is the P2 trade measured again from the other side: the ≈4 MiB per slot
*is* the fresh world's price of not re-faulting an image.

**Decision.** The default stays 8 MiB; there is no cheaper point on the
curve between it and 0. `USAI_WASM_KEEP_RESIDENT=0` is documented as the
operator knob for a box whose bursts are rare and whose memory limit is
tight — with its price stated (≈2× the per-request CPU, half the throughput
on one vCPU) — and the sizing rule for everyone else is `40 MiB + max_worlds
× 4 MiB`, or a lower `--max-worlds`. No application-facing configuration
(AGENTS.md §8). The hello cells also note that, on this morning's host, c=4
gave 1 800 req/s where yesterday's run gave 2 222 on the same cell: the 72 h
soak is a heavier co-tenant than the 24 h one was at that hour; same-day
pairs are the comparison.

## 5. Density — N applications on one host

Bare processes on cpus 12–15, N = 1 / 5 / 10 / 25 / 50 tenants of the
template (distinct artifacts from the same source), each with
`--max-worlds 16`, its own status listener and `pool.max` 4 against the
shared PostgreSQL. Ten minutes per cell: 60 s settle, then 45 of 50
applications receive **nothing**, 4 receive 1 req/s (alternating hello and
the PostgreSQL read), and application 1 bursts **100 req/s for 10 s, twice**
(at the ⅓ and ⅔ marks). Then a cold first response is measured on an
application that has been idle for the whole run. The same driver runs
Node + Fastify.

**Idle fleet (the 60 s settle window; PSS is the honest column):**

| N | Usai RSS Σ | Usai PSS Σ | Node RSS Σ | Node PSS Σ | Usai per app RSS / PSS | Node per app RSS / PSS | idle CPU Usai / Node |
|---|---|---|---|---|---|---|---|
| 1 | 47 | 40 | 82 | 50 | 47.1 / 39.8 | 81.6 / 50.0 | 0.00 % / 0.00 % |
| 5 | 235 | 162 | 421 | 210 | 47.0 / 32.4 | 84.2 / 42.0 | 0.00 / 0.02 |
| 10 | 469 | 311 | 845 | 392 | 46.9 / 31.1 | 84.5 / 39.2 | 0.00 / 0.03 |
| 25 | 1 174 | 753 | 2 095 | 907 | 47.0 / 30.1 | 83.8 / 36.3 | 0.00 / 0.03 |
| **50** | **2 352** | **1 493** | **4 212** | **1 793** | 47.0 / **29.9** | 84.2 / **35.9** | **0.02 / 0.12** |

Least-squares fits (PSS Σ vs N): Usai ≈ **29.6 MiB per application + 13 MiB**,
Node ≈ **35.3 MiB per application + 27 MiB**. Both curves are linear; nothing
super-linear appeared in either fleet up to 50. The per-application PSS
falls with N in both because the shared pages (the binary and the
runtime's images for Usai; the Node binary and ICU for Node) are divided
among more processes.

Idle CPU: the fifty Usai processes together consumed **0.02 % of one core**
(2 minor faults per second, 700 fds, 455 threads); the fifty Node processes
0.12 %. Threads per process: Usai 9, Node 7. fds: Usai 14, Node 22.

**The burst (application 1, 100 req/s for 10 s, `GET /hello/burst`):**

| N | Usai p50 / p99 ms | Usai CPU % during burst | Node p50 / p99 ms | Node CPU % during burst | Usai memory after both bursts | Node memory after both bursts |
|---|---|---|---|---|---|---|
| 1 | 1.29 / 1.97 · 1.16 / 2.05 | 12.5 · 11.1 | 0.54 / 1.34 · 0.49 / 0.91 | 7.1 · 5.5 | RSS +0.4, PSS +0.4 | RSS −2.2, PSS −2.2 (GC) |
| 10 | 1.16 / 1.93 · 1.13 / 1.90 | 11.7 · 11.1 | 0.54 / 1.14 · 0.48 / 0.88 | 7.0 · 5.2 | +0.4 | −83 (fleet, GC) |
| 50 | 1.24 / 1.96 · 1.25 / 2.42 | 12.5 · 13.0 | 0.57 / 1.39 · 0.51 / 1.07 | 7.4 · 5.2 | +0.4 | −500 (fleet, GC) |

Zero errors in every burst on both sides. Per request, at 100 req/s, Usai
spends **≈1.2 ms of CPU** (12.5 % of a core) where Node spends ≈0.7 ms —
the P8 finding restated: the fresh world plus the interpreter running the
developer surface is ≈1.7× a warm V8 process on a trivial route. The Node
fleet's RSS *falls* during the burst because V8 runs a GC it had deferred;
the Usai fleet is flat to within 0.4 MiB because the bursting application
had already touched its slots in the settle window.

**Cold first response** (an application idle for the whole run, one
`GET /users/42` through PostgreSQL): Usai **10.8 / 16.1 / 13.7 ms** at
N = 10 / 25 / 50; Node **45.9 / 44.4 / 61.7 ms**. A Usai application that
has done nothing for ten minutes answers in the time its first world takes
to instantiate; a Node application pays for the JIT's cold code and a lazy
pool connection.

**Verdict.** Per idle application Usai costs ≈30 MiB PSS (47 RSS) and no
CPU; Node + Fastify ≈36 MiB PSS (84 RSS) and 0.12 % of a core across fifty.
That is a real but **modest** memory advantage (−17 % PSS, −44 % RSS), a
decisive idle-CPU and cold-start advantage, and a **CPU-per-request
disadvantage** (≈1.7×) that P8 already itemised. Fifty idle Usai
applications fit in 1.5 GiB of proportional memory; fifty Node ones in
1.8 GiB. A claim of "an order of magnitude denser" is **not** supported by
these numbers; "cheaper when idle, more expensive per request, faster from
cold" is.

## 6. What P8E changed in the runtime (idle costs only)

- **`crate::idle`** — the watchdog (5 ms) and the epoch ticker (10 ms)
  park on a condvar while no world is live and wake on the first
  `WatchSlot::register`. Before: ≈300 wakeups/s and ≈110 ms of CPU per 20 s
  in an idle process (the whole idle `perf` profile); after: 0.00 % in
  every idle cell and 0.02 % for fifty processes. No lifecycle test
  changed; the deadline, CPU-slice and cancellation suites are the guards.
- **`/_usai/status` `process` block and `usai_process_*` gauges** — RSS,
  PSS, VmSize, peak RSS, page faults, CPU seconds, threads, fds, read from
  `/proc/self` on request (nothing sampled in the background).
- **The sampler, the fleet driver and the template** under
  `scripts/qualification/p8e/`, reusable for the next envelope.

Not changed: the residency default (§4 decides), image residency (C3 — PSS
shows the image shared across slots already; no case), the pool floor (C4 —
`pool.max` 4 per application gives 200 connections at N=50; the arithmetic
is in `SUPPORTED.md`).

## 7. What this says about P9

P8 removed what the implementation paid that the semantics never asked for;
the remaining ≈0.6 ms per hello request over Node at c=1 is the interpreter
running the developer surface (≈0.26 ms), the boundary JSON (≈0.15) and the
fresh world (≈0.16). P8E adds the other side of the ledger: at idle Usai is
cheaper than Node in memory and far cheaper in CPU, and it comes back from
idle 4× faster.

For a host that runs **many mostly-idle applications** — the shared-hosting,
internal-tools, per-tenant-process shape — the envelope is already
favourable and a hot-path P9 would not change the operator's bill. For a
host that runs **one busy application**, CPU per request is the bill, and
Usai pays ≈1.7× Node on trivial routes (much less on PostgreSQL routes,
where the query dominates: P8 classes C–F). The concurrency sweep on the idle VM (started 2026-09-23, after the 72 h
soak) is the third number the P9 decision needs; the
recommendation so far is: **P9 is worth opening only for the "one busy
application" case, and only on the two levers that are not the model
itself** — the boundary JSON (a typed encoding in place of one JSON parse
per direction) and the interpreter's share of the developer surface (a
smaller per-request SDK path in the snapshot). The fresh world is the
product; its ≈0.16 ms is not on the table.

## 7b. The comparators' floors: attempted 2026-09-23, not obtained

The sweep day ended with `fleet.sh floor node|php` at the cell sizes the
Usai floors used (node 48/64/96/128 MiB, php 32/48/64/96, all 1 vCPU) plus
the template at 192 MiB / 1 vCPU / 48 worlds as the reference. **Only the
reference cell is a result**; both comparator paths are broken in ways that
are ours, and are reported rather than published:

- **node**: every cell ended in under a second leaving only a container id
  — no readiness verdict, no samples, no `server.log`. The path has never
  produced a cell; it is a harness bug, not a Node result.
- **php**: the four cells ran for eleven minutes each and gave *identical*
  numbers (idle 86.9 MiB, load 145.1 MiB, 118 % of one cpu) — the cell's
  `mem_limit` and `cpus` did not bind on what was measured, so a "32 MiB
  floor" that idles at 87 MiB is not a floor. The load phase also carried
  6 977 × 5xx from the same nginx ephemeral-port exhaustion the sweep hit.

What the reference cell does say, on the same host and the same measurement
(one vCPU, class C at c=16 for 60 s between two idle phases):

| | Usai (template, 48 worlds) | PHP-FPM tuned (8 children + nginx) |
|---|---|---|
| processes | 1 (3–4 threads) | 11 |
| idle RSS / PSS | 40.6 / 40.6 MiB | 86.9 / — MiB |
| under load | 101.3 / 59.0 MiB | 145.1 MiB |
| after 300 s idle | 102.3 / 59.2 MiB | 145.1 MiB |
| cpu during load | 99 % of the core | 118 % (the limit did not bind) |
| load result | 1 vCPU class C, no 5xx | 585 req/s with 6 977 × 5xx |

Neither the node harness bug nor the php limit is worth a rerun before the
comparator configuration is fixed; both are tracked with the sweep's own
follow-ups (`2026-09-23-sweep.md` → what this run does not measure).

## 8. Open

- **A multi-application runtime process** (many definitions, one process,
  one pool) is the only way to make the per-application intercept — the
  ≈30 MiB an idle process holds — go away; it is an architecture change
  that `GOAL.md` §52 lists as research-grade (memory reservation policy for
  many-app density) and is recorded in `docs/OPEN-QUESTIONS.md`, with these
  numbers as its motivation: 50 × 30 MiB = 1.5 GiB where the definitions
  themselves are a few MiB.
- A residency policy between "keep everything touched" and "decommit
  everything" — an idle-time decommit (return a slot's pages after N
  seconds unused, pay the faults only on the first world after a quiet
  period) would give a small box both curves; not attempted here (P8E fixes
  only measured idle costs), listed for P9 if the sweep says the per-slot
  4 MiB matters to anyone.
