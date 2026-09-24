# Overload and capacity refusals

**First command**: `usai top --addr <status listener>`. Refusals never reach
a workload, so they are in no row of any per-route table — `usai top` puts
them on their own line (`rejected before a world: capacity 42.0/s`), beside
the `live` column that says which workload is holding the world budget and
the pool's `in use` / `waiting`. That is the whole shape of this incident on
one screen.

## What you see

- Responses: **503 `capacity_exhausted`** as soon as every world slot the
  admission budget allows is busy; the refusal costs no world (it is decided
  before one exists) so it stays cheap under any load. **The body names the
  level that is full and how much of it is in use** — read it before changing
  anything:

  ```json
  {"error":{"code":"capacity_exhausted","message":"runtime.worlds budget exhausted (2 in use)"}}
  ```

  `runtime.worlds` is `--max-worlds`; a workload's own name there is its
  `concurrency:`; a resource's is its pool. `usai inspect` prints all four
  levels under **Admission**.
- Measured behaviour under real saturation (2026-09-23, c=64 → 512 on the
  qualification VM, `docs/measurements/2026-09-23-saturation-and-queue.md`):
  throughput stays flat and p99 grows with the concurrency until the budget
  is reached, and past it the instance refuses rather than degrading — no
  fault, no queue inside a world, and the server log stays silent because a
  refusal is a counter and not a line.
- Metrics: `usai_http_rejections_total{reason="capacity"}` rises;
  `usai_worlds_live` sits at the bound; `usai_http_request_seconds` p99 for
  admitted requests stays bounded because nothing queues inside the runtime.
- Log: nothing per refusal (they are counted, not logged).

## What to change

- More capacity on this instance: `usai run --max-worlds <n>`
  (`USAI_MAX_WORLDS`); each world reserves memory up front (see
  memory-pressure). Per-workload bounds come from the application
  (`concurrency` on a workload).
- More instances behind the proxy: the runtime is stateless; run the cron
  scheduler on exactly one instance (`--no-cron` / `USAI_NO_CRON=1` on the
  others), queue consumers wherever you want the work done (`--no-queue`
  dedicates replicas), services where you mean them (`--no-services`).
- A slow dependency, not traffic: `usai_resource{metric="in_use"}` at `max`
  means the pool is fully *used*; `usai_resource{metric="waiting"}` above 0
  means worlds are **queueing for a connection**, which is the one that
  needs a bigger `pool.max` (the other needs a faster query). A world that
  waits longer than `pool.acquireTimeoutSeconds` (10 s) is refused with
  `503 resource_exhausted` — before 0.0.10 it waited for as long as its
  deadline allowed, which is how a 2-connection pool answered 150 clients
  with 200s at a median of 23 seconds. `slow-route.md` walks the whole
  diagnosis.

The campaign's spike (64 clients on top of 8) produced no refusals at 256
worlds: p99 rose to 57 ms and throughput to the CPU's limit.
