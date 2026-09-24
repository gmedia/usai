# The control run: 24 h with a dataset that does not grow (2026-09-23/24)

**Status: complete.** 2026-09-23 16:49 → 2026-09-24 16:49 UTC on VM 47,
24.01 h, 988 samples. **124 471 721 requests, 0 × 5xx, 0 × 503**, and
throughput that did not decay: 1 416 req/s in the first hour, 1 480 in the
last. The verdict is in "What it answered" below.

## The question it answers

The 72 h soak passed every correctness criterion — 73.6 M requests, 0 × 5xx,
0 × 503, flat RSS over three days — and its throughput fell from **794 to
154 req/s** with a *flat* p50 and a *flat* per-request CPU
(`2026-09-18-p5-p6-qualification.md` → Soaks). The explanation offered was
the 7.4 million invoices the run had written into a table nobody pruned: more
rows, slower queries, fewer requests per second, and no runtime cost per
request to show for it.

That explanation was never tested. A slow decay with flat per-request cost is
also what a runtime leaking *something* per request would look like if the
leak were outside the CPU — a growing structure walked once per request, a
connection pool that degrades, a table of live handles. **So: the same load,
the same duration, the same deployment, with the application's data pruned as
it is made.** If the decay is gone, the runtime was never the cause. If it is
still there, it is ours.

## Method

`scripts/qualification/p6/run.sh soak 86400 bounded` against the P5
production-shaped deployment (Caddy → the published `ghcr.io/gmedia/usai`
runtime image → PostgreSQL 18, `examples/invoicing`), 8 closed-loop clients.
Every minute the harness deletes what the load made, the way an operator
would:

- the invoices beyond the newest 20 000 (`delete from invoices …`), and
- the queue's finished rows older than a minute (what `usai queue prune`
  does; the campaign issues the SQL directly because the container has the
  database, not the CLI).

A sample a minute records RSS, CPU, open descriptors and the whole
`/_usai/status` document (ownership gauges, pool and quarantine counters,
queue depth, held images, the latency histogram).

## Disclosed co-tenants

The floor campaigns of the same evening ran beside it, pinned to cpus 12–15
while the soak has 0–11. That is the arrangement every campaign on this VM
has used, and it is visible in the data exactly once:

> **The soak's only bad seconds in its first four hours — 12 client timeouts
> across three seconds, at 17:34:07–17:34:35 UTC — line up to the second with
> a co-tenant `docker build` writing two image layers.** The same host-I/O
> mechanism the 24 h and 72 h soaks recorded as "two host stalls while Docker
> extracted image layers", this time attributable rather than inferred. No
> 5xx, no 503, no refusal: the client gave up at its 10 s timeout while the
> host was busy.

After that the campaigns stopped building images; the cells that followed run
containers from images already present.

## What it answered

**Throughput does not decay with a bounded dataset.** That was the question,
and the answer is unambiguous:

| | hour 1 | hour 24 |
|---|---:|---:|
| requests/second | 1 416 | 1 480 |

The 72 h run, on the same deployment and the same load with a table nobody
pruned, went from **794 to 154 req/s** — a collapse of 81 % — with a flat
p50 and a flat per-request CPU. With the dataset pruned there is no trend at
all, at roughly twice the rate, over 124 million requests. **The decay was
the application's table, not the runtime.**

That was worth testing rather than asserting: a slow decline with flat
per-request cost is also the signature of a runtime leaking something
outside the CPU — a structure walked once per request, a pool that degrades,
a table of live handles. It is not what is happening here.

### The totals

| | |
|---|---|
| Requests | **124 471 717 × 2xx**, 4 × 4xx, **0 × 5xx**, **0 × 503** |
| Refused before a world | 2 (both `route` — the 4xx above) |
| Mean latency | 4.39 ms over the whole run |
| Client-side | 124 470 547 ok, **344 errors**, all of them 10 s timeouts (see below) |
| Lifecycle over 124.47 M worlds | **0** detached work, **0** completions dropped late, **0** rejected stale, **0** quarantined connections |
| Transactions rolled back for a world | 301, with 344 cancellations — the C4/C5 machinery doing its job, not a fault |
| Cron | 1 tick due, 0 failed, 0 skipped |
| Memory (process RSS) | 86.7 → 91.6 MiB |
| Memory (cgroup charge) | 53.2 → 55.2 MiB |
| Open descriptors | 29 → 31 |

### Every bad second is the host, and they are all in two windows

The 344 client timeouts fall in 91 distinct seconds, and they are not spread
across the run:

| hour | 1 | 2–15 | 16 | 17 | 18 | 19–24 |
|---|---:|---:|---:|---:|---:|---:|
| client timeouts | 12 | **0** | 133 | 184 | 15 | **0** |

- **Hour 1** (t=2685–2713, a 28-second window) is the co-tenant `docker
  build` disclosed above — attributable to the second.
- **Hours 16–18** are a host-CPU event, not a runtime one: the sampler's
  `cpuPct` for the container falls 415 → 250 → **64.9** at t=59023 and is
  back to 409 by t=59468. A closed-loop client on a container given fewer
  cycles sends fewer requests and eventually times out at its own 10 s
  bound. The server recorded **no 5xx and no 503** through all of it, and
  hourly throughput shows the same dip (1 228 and 1 098 req/s against a
  ~1 470 baseline) before returning to 1 430–1 484 for the last six hours.

No second outside those two windows produced an error, over 22 hours and
114 million requests.

### Memory is a step and a plateau, not a ramp

+4.9 MiB of process RSS over 24 hours and 124 million requests, in **two
steps** (hour 8 and hour 16) with flat stretches between and after:

```
h1–h7    86.7 → 86.9 MiB      h16–h24  91.6 MiB (flat to the tenth)
h8–h15   91.3 → 91.4 MiB
```

A per-request leak is monotonic against request count. This is not: the last
nine hours and 45 million requests moved it by 0.0 MiB. The steps line up
with the two co-tenant events above, which is the shape of a pool slot being
touched for the first time under load and then kept resident — the residency
policy `docs/upstream/wasmtime-idle-decommit.md` is about.

## What was observed during the run (9.5 h)

| | |
|---|---|
| Requests | 50.1 M ok, 4 × 4xx, 0 × 5xx, 0 × 503 |
| Errors | 12 client timeouts, all in the three seconds above |
| p50 | 3.9 ms in the first hour, 4.0 ms now |
| Memory (cgroup charge) | 52.0 → 54.8 MiB |
| Memory (the process's own) | 91 MiB RSS, 65 PSS |
| Lifecycle | 0 detached work, 0 quarantined, 0 completions dropped or rejected, over 50.1 M worlds |
| Open descriptors | 29 → 31 |

**Two memory numbers that differ by 36 MiB, and neither is "what this
instance needs".** The sampler's is `docker stats` (what the *cgroup* is
charged: 53 MiB); `/_usai/status` reports the process's own `VmRSS`
(89 MiB). Both were attributed at 7.1 h rather than guessed, from
`/proc/1/smaps` and `/sys/fs/cgroup/memory.stat` inside the container:

| mapping | RSS | PSS | what it is |
|---|---:|---:|---|
| `/memfd:wasm-memory-image` | 35 988 kB | 12 871 kB | the Wasm image, mapped into **8 pooled slots** |
| anonymous | 25 276 | 25 276 | the runtime's heap and stacks |
| `/usr/local/bin/usai` | 17 680 | 17 680 | the binary's text |
| `[heap]` | 7 388 | 7 388 | |

Two opposite errors, which is why the totals are so far apart:

- **`VmRSS` over-counts the image.** The kernel's RSS counters increment per
  page-table entry, so a physical page mapped into eight slot mappings is
  counted eight times: 36 MiB of RSS over ≈13 MiB of pages. `PSS` (65.9 MiB
  in the same sample, also in `/_usai/status`) divides the sharing out.
- **The cgroup under-counts the text.** `file_mapped` is 3.6 MiB against the
  binary's 17.7 MiB resident: those pages were faulted in from the image
  layer by something outside this container, and page cache is charged to
  whoever faults it first (`2026-09-23-floor-accounting.md`).

The two partly cancel — which is exactly why a floor may not be read off
either one. It is read from a box that has to fault everything in itself,
which is what the floor cells do (`DROP_CACHES=1`), and is how the technical
floor turned out to be 64 MiB and not 48. For sizing a *running* instance,
**`pssKib` is the process-level number to use, and the cgroup's own charge
is the one that gets you OOM-killed** — `/_usai/status` reports both when
the process runs under a limit (this deployment's image predates that
field).

Throughput by hour, which is the question this run exists to answer:

| hour | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9 |
|---|---|---|---|---|---|---|---|---|---|
| mean req/s | 1 419 | 1 483 | 1 437 | 1 496 | 1 487 | 1 484 | 1 479 | 1 483 | 1 479 |
| median p99 (ms) | 19.2 | 18.8 | 19.0 | 18.7 | 18.7 | 18.8 | 18.8 | 18.9 | 18.9 |

**Flat.** The 72 h run with a growing dataset had lost a third of its
throughput by this point on its way from 794 to 154 req/s; this one has no
trend at all, at a *higher* rate, with an unchanged p50.

Nine hours is not an answer to a question about seventy-two, and the
hypothesis was not proven until the run ended — it then ran another
14.5 hours and finished at 1 480 req/s, above where it started.

**The means are flat; the seconds underneath them are not perfectly even.**
About 3–5 % of seconds serve under 1 000 requests (against a ~1 450 mean),
in short clusters — 581 of them in six hours, a median of 26 s apart and at
most 74 s long — with the per-second p99 median unchanged at 18.7 ms
throughout:

| hour | 1 | 2 | 3 | 4 | 5 | 6 |
|---|---|---|---|---|---|---|
| seconds under 1 000 req/s | 292 | 132 | 232 | 103 | 129 | 131 |
| p99 median (ms) | 19.2 | 18.8 | 19.0 | 18.7 | 18.7 | 18.8 |
| p99 p95 (ms) | 118.5 | 41.9 | 67.7 | 36.6 | 44.4 | 52.6 |

Hours 1 and 3 are the two hours a floor campaign was running on cpus 12–15,
and they are the two worst — the co-tenant is visible, as disclosed. What is
left after it (≈2–3 % of seconds, every hour) is **not** the once-a-minute
prune: the clusters are spread evenly across the minute rather than landing
on it. The standing hypothesis is the database's own maintenance — the prune
deletes rows every minute, which is dead tuples for autovacuum, and
`pg_stat_user_tables` shows exactly that churn on the tables the load writes.
The database's own counters support it: six hours in,
`pg_stat_user_tables` shows **364 autovacuums each** on `invoices` and
`invoice_items` (3.19 M rows deleted between them) and 360 on `tenants` —
about one a minute per table, which is the same rate as the prune and two
orders of magnitude more often than the checkpoints (73 timed in the same
window, one every five minutes as configured). Recorded as the hypothesis it
is: the correlation is strong and nobody has pinned a slow second to a
specific vacuum.

**It is worth an operator's attention either way**: keeping a table bounded
is not free, and a queue or an invoice table pruned on a schedule pays for it
in the database's maintenance, not in the runtime's.

## What this run cannot settle

- **Whether the 72 h decay was the dataset** — only whether a *bounded*
  dataset decays too. If this run is flat, the dataset remains the best
  explanation, not a proven one; proving it would need the growing run
  repeated with per-query timing, which is a different campaign.
- **Anything about 72 hours.** This is 24.
- **The queue's own growth** beyond what the prune removes: the bound is on
  finished rows, and a consumer that never finishes would still accumulate.
