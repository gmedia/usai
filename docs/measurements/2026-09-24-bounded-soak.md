# The control run: 24 h with a dataset that does not grow (2026-09-23/24)

**Status: running.** Started 2026-09-23 16:49:22 UTC on VM 47, ends
2026-09-24 16:49 UTC. This file carries the method and what has been observed
so far; the verdict is appended when it finishes.

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

## What has been observed so far (3.9 h)

| | |
|---|---|
| Requests | 16.8 M ok, 0 × 4xx, 0 × 5xx, 0 × 503 |
| Errors | 12 client timeouts, all in the three seconds above |
| p50 | 4.6 ms at the start, 4.1 ms now |
| Throughput | ≈1 400–1 580 req/s, no trend yet |
| RSS | 52.0 → 53.2 MiB |
| Open descriptors | 29 → 30 |
| Worlds created | 20.5 M |

Four hours is not an answer to a question about a 72-hour decay; it is
recorded here so the shape is on paper before the verdict is.

## What this run cannot settle

- **Whether the 72 h decay was the dataset** — only whether a *bounded*
  dataset decays too. If this run is flat, the dataset remains the best
  explanation, not a proven one; proving it would need the growing run
  repeated with per-query timing, which is a different campaign.
- **Anything about 72 hours.** This is 24.
- **The queue's own growth** beyond what the prune removes: the bound is on
  finished rows, and a consumer that never finishes would still accumulate.
