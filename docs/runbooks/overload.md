# Overload and capacity refusals

## What you see

- Responses: **503 `capacity_exhausted`** as soon as every world slot the
  admission budget allows is busy; the refusal costs no world (it is decided
  before one exists) so it stays cheap under any load.
- Metrics: `usai_http_rejections_total{reason="capacity"}` rises;
  `usai_worlds_live` sits at the bound; `usai_http_request_seconds` p99 for
  admitted requests stays bounded because nothing queues inside the runtime.
- Log: nothing per refusal (they are counted, not logged).

## What to change

- More capacity on this instance: `usai run --max-worlds <n>`
  (`USAI_MAX_WORLDS`); each world reserves memory up front (see
  memory-pressure). Per-workload bounds come from the application
  (`maxConcurrency` on a workload).
- More instances behind the proxy: the runtime is stateless; only the
  cron scheduler and queue consumers should run on one instance per
  application (`cron_scheduler`/`queue_consumers` in the runtime
  configuration).
- A slow dependency, not traffic: if `usai_resource{metric="in_use"}` is at
  `max` while worlds are live, the pool is the bottleneck (503
  `resource_exhausted`); raise `pool.max` or fix the query.

The campaign's spike (64 clients on top of 8) produced no refusals at 256
worlds: p99 rose to 57 ms and throughput to the CPU's limit.
