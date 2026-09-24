# Metrics reference

Every metric `GET /_usai/metrics` exposes (Prometheus text; the status
listener, `--status-addr`, or the application listener with `--status`;
behind `USAI_STATUS_TOKEN` when set). Generated from a running
`examples/invoicing` on 2026-09-20; when a name here and the exposition
disagree, the exposition is right and this page is stale.

Two conventions: **levels are gauges, events are counters** (`usai_resource`
is the pool's current level, `usai_resource_quarantines_total` the number of
quarantines ever — alert on `increase()` of the second, never on the first
as if it were cumulative), and **refusals decided before a world existed are
counted separately** (`usai_http_rejections_total{reason}`) and are *not* in
`usai_http_request_seconds`: a p99 that improves during a flood of bad
requests is a trap, read the rejections beside it. The same fact bites
elsewhere: **`usai_http_request_seconds_count` is not the request rate** —
it counts admitted requests only, so a dashboard that measures throughput
with `rate(usai_http_request_seconds_count[5m])` under-reports by the
refusals and *falls* while a bad-traffic flood rises. Volume is
`usai_http_requests_total`; the histogram is for latency.

| Metric | Type | Labels | Meaning |
|---|---|---|---|
| `usai_scheduler` | gauge | kind | 1 when this instance runs the scheduler of that kind (cron: exactly one replica should) |
| `usai_worlds_live` | gauge | — | Execution worlds currently alive |
| `usai_worlds_created_total` | counter | — | Execution worlds created |
| `usai_guest_cpu_seconds_total` | counter | — | Thread CPU time spent executing guest code, summed over worlds |
| `usai_workload_cpu_seconds_total` | counter | workload | The same, per workload. CPU is accounted, not scheduled (ADR-0021): this is how you find the workload whose share does not match its importance, and `concurrency:` plus a deadline is how you cap it |
| `usai_ops_live` | gauge | — | External operations with a live owner |
| `usai_completions_total` | counter | outcome | Operation completions by routing outcome |
| `usai_detached_work_total` | counter | — | Finite worlds that ended with live asynchronous work |
| `usai_world_budget` | gauge | kind | Runtime world budget |
| `usai_revision_in_flight` | gauge | application, revision, state | Work in flight per revision |
| `usai_service` | gauge | revision, service, state | 1 per declared `service()`, at its current state (absent when the application declares none) |
| `usai_queue_messages_total` | counter | revision, state | Queue messages by outcome, per revision |
| `usai_cron_ticks_total` | counter | revision, state | Cron ticks per revision: due on this instance, skipped (previous still running), failed, taken by another instance (exclusive schedules) |
| `usai_http_workload_request_seconds_sum` | counter | workload | Summed response time per workload. **`rate(sum)/rate(count)` is how you find a slow route** — the histogram below has no workload label, so a slow route that is a minority of traffic never moves its p99 (`slow-route.md`) |
| `usai_http_workload_request_seconds_count` | counter | workload | Responses timed per workload (the denominator) |
| `usai_resource_operations_total` | counter | kind, name | Operations leased from the resource |
| `usai_resource_transactions_total` | counter | kind, name | Transactions opened on it |
| `usai_resource_requests_total` / `_failures_total` / `_refused_total` | counter | kind, name | Outbound requests through an `httpClient` resource: made, failed, refused by its own in-flight bound |
| `usai_resource` | gauge | kind, metric, name | Resource manager state (current levels): `in_use`, `max`, `waiting` (worlds queued **for a connection**, not for the database — the one number that tells a small pool from a slow dependency), `ready` (1 while the last contact with the resource succeeded, 0 after a connection-level failure until the next success) |
| `usai_resource_quarantines_total` | counter | kind, name | Connections quarantined because their outcome could not be proven (cumulative) |
| `usai_tasks` | gauge | state | Dispatched task queue |
| `usai_build_info` | gauge | version | Usai runtime version (label), always 1 |
| `usai_process_start_time_seconds` | gauge | — | Unix time the runtime started |
| `usai_process_resident_memory_bytes` | gauge | — | Resident set size. **It over-counts this runtime**: the pooled Wasm image is one set of physical pages mapped into every world slot, and the kernel's RSS counters increment per mapping — measured on the 24 h bounded soak, 89 MiB RSS over 66 MiB PSS and 53 MiB charged to the cgroup. Use it for *trend* (rising while `usai_worlds_live` does not is a leak); use PSS or the charged bytes below for a *level* |
| `usai_process_proportional_memory_bytes` | gauge | — | Proportional set size (shared pages divided among their sharers): the honest per-process footprint |
| `usai_process_memory_limit_bytes` | gauge | — | The cgroup memory limit the process runs under. Published only when there is one — and when there is, all four cgroup series are published, zeros included, so `ceiling_hits 0` means *measured and zero*. In `/_usai/status` the same fields are omitted when they are zero, so **`memoryLimitBytes` being present is what says the group was measured** |
| `usai_process_memory_charged_bytes` | gauge | — | What the cgroup is charged (`memory.current`): resident memory **plus the page cache this container faulted in**, which is what the limit actually bounds |
| `usai_process_memory_ceiling_hits_total` | counter | — | Times the cgroup hit its limit and had to reclaim (`memory.events` `max`). **Alert on this**: climbing means the container is at its ceiling and thrashing on its own pages — no OOM kill, no log line, and throughput collapses (`memory-pressure.md`) |
| `usai_process_memory_oom_kills_total` | counter | — | OOM kills inside this cgroup. Normally 0 — a kill of the runtime takes the metric with it — so a non-zero value means a child process was killed |
| `usai_process_virtual_memory_bytes` | gauge | — | Virtual size (address space reserved per world slot up front; not resident). **Do not alert on it and do not cap it**: it is hundreds of gigabytes by design — ≈208 GB at `--max-worlds 48` — because each slot reserves a guarded 4 GB range so the engine can elide bounds checks. `LimitAS` / `ulimit -v` kills the process at the first world. Resident memory is `usai_process_resident_memory_bytes`, and the limit that means something is `MemoryMax` / `mem_limit` |
| `usai_process_resident_memory_peak_bytes` | gauge | — | Peak resident set size since start |
| `usai_process_page_faults_total` | counter | kind | Page faults since start, by kind |
| `usai_process_cpu_seconds_total` | counter | — | CPU consumed since start, user + system |
| `usai_process_threads` | gauge | — | OS threads |
| `usai_process_open_fds` | gauge | — | Open file descriptors |
| `usai_http_workload_responses_total` | counter | class, workload | HTTP responses by workload and status class (refusals after routing included) |
| `usai_http_requests_total` | counter | — | HTTP requests received |
| `usai_http_responses_total` | counter | class | HTTP responses by status class |
| `usai_http_rejected_before_world_total` | counter | — | Requests refused before any world existed |
| `usai_http_rejections_total` | counter | reason | Requests refused before a world existed, by reason |
| `usai_http_request_seconds` | histogram | le | Time to the response, by bucket |
| `usai_http_upgrades_total` | counter | — | WebSocket upgrades |
| `usai_http_streams_total` | counter | — | Streaming responses |
| `usai_http_streams_failed_total` | counter | — | Streams whose handler failed after the 200 and the head were sent (the body ended early) |

Label values:

- `usai_scheduler{kind}`: `cron`, `queue`, `services` — 1 when this instance
  runs that scheduler; `sum by (kind) (usai_scheduler{kind="cron"})` across
  replicas must be 1 (`SUPPORTED.md`: cron ticks on every instance that
  schedules).
- `usai_world_budget{kind}`: `in_use`, `max` (`--max-worlds`).
- `usai_completions_total{outcome}`: `delivered`, `dropped_late`,
  `rejected_stale` — a completion that arrived after its world ended is
  dropped or rejected by design (C4), not lost work.
- `usai_revision_in_flight{state}`: `installed`, `active`, `draining`,
  `retired` — lowercase here and in `/_usai/status` alike.
- `usai_service{state}`: `starting`, `running`, `stopping`, `stopped`,
  `failed` — `failed` stays 1 when the restart policy is exhausted (log line
  `service gave up`); it does not make the instance unready.
- `usai_http_workload_responses_total{workload}`: the workload id,
  `<kind>:<name>` as `usai inspect` lists it — for an HTTP or stream route
  the **method and path**, `http:GET /invoices`, `http:POST /invoices/:id/pay`
  (mind the space: quote the label value in PromQL); for the other kinds the
  declared name, `socket:chat`, `task:send-reset-email`. One series per
  workload that has answered; a request that never matched a route is under
  `usai_http_rejections_total{reason="route"}` and has no workload.
- `usai_queue_messages_total{state}`: `claimed`, `done`, `retried`, `dead`,
  `invalid`, `reclaimed` (claimed by a consumer that died, returned for
  another attempt or dead-lettered). Per instance and revision, cumulative.
  **Queue depth is not a metric** (it would cost a query per scrape):
  `usai queue status` asks the database for it — rows per topic and state,
  the oldest message still waiting (the lag) and the oldest still claimed (a
  lost consumer, if it is older than the consumer's deadline). `--json` for a
  script. The SQL behind it is
  `select topic, state, count(*) from usai_queue group by 1, 2`, if you would
  rather scrape it yourself.
- `usai_cron_ticks_total{state}`: `due` (ticks this instance's scheduler reached), `skipped` (previous invocation still running, `overlap: skip`), `failed`, `taken` (an `exclusive` schedule's tick another instance claimed first). Per revision, cumulative.
- `usai_resource{kind,name,metric}`: `kind` = `postgres`, `http.client`,
  `cache.local`; `metric` = `in_use`, `max`.
- `usai_tasks{state}`: `queued`, `running`, `completed`, `failed`, `lost`
  (dispatched in memory and gone with a shutdown — ADR-0010).
- `usai_http_rejections_total{reason}`: `route`, `validation`, `auth`,
  `capacity`, `draining`, `other`. `draining` is rare by design: a request
  that was routed by a revision which retired between routing and
  admission (`503 revision_draining`). A process stop does **not** produce
  it — the listener keeps serving during the grace and then closes, so
  requests are answered or refused at the socket, never rejected here.
- `usai_http_responses_total{class}` and
  `usai_http_workload_responses_total{workload,class}`: `2xx`, `3xx`, `4xx`,
  `5xx`; a WebSocket upgrade (101) is a success and counts in
  `usai_http_upgrades_total`, a committed stream in `usai_http_streams_total`.
  A request cancelled by its client's disconnect or by the drain bound is
  recorded as **499** (`4xx`, code `cancelled`): the client did not receive
  it, the log has `request cancelled` at debug. It is not a 5xx because the
  runtime did not fail; count `increase(…{class="4xx"})` around a restart
  with that in mind.
- `usai_process_page_faults_total{kind}`: `minor`, `major`.

A **Grafana dashboard built from this page** is
`docs/deploy/grafana-dashboard.json` — import it, pick the datasource and the
job. Sixteen panels, every query taken from the table above, including the
two things nothing else shows you: what the cgroup is charged against its
limit, and the ceiling hits that mean the container is reclaiming the pages
it executes from.

## Alerts worth setting from this page alone

| Alert | Expression (PromQL shape) | Why |
|---|---|---|
| Capacity refusals | `increase(usai_http_rejections_total{reason="capacity"}[5m]) > 0` | the instance is refusing work before a world exists: raise `--max-worlds`, add a replica, or find the slow dependency (`overload.md`) |
| Database unreachable | `usai_resource{kind="postgres",metric="ready"} == 0` for 30 s | the last operation or probe failed at the connection level; `/_usai/status` → `resources[].detail.lastError` says how (`postgres-down.md`) |
| Pool poisoned / database flapping | `increase(usai_resource_quarantines_total[5m]) > 0` while the database is healthy | a connection's outcome could not be proven (C5); a burst during a failover is expected, a steady trickle is a bug or a network problem (`postgres-down.md`) |
| Pool saturated | `usai_resource{metric="waiting"} > 0` for 1 m | requests are queueing **for a connection**: size `pool.max` against `--max-worlds`, or make the queries faster. `in_use == max` on its own is healthy saturation and is not worth waking anyone for — read `waiting` beside it (`slow-route.md`) |
| A route got slow | `topk(5, rate(usai_http_workload_request_seconds_sum[5m]) / rate(usai_http_workload_request_seconds_count[5m])) > 0.5` | per-workload mean latency. The global histogram's p99 cannot see a slow route that is a minority of traffic — measured at 2.5 ms while a route took 204 ms |
| Detached work | `increase(usai_detached_work_total[1h]) > 0` | an application bug: a handler returned with work in flight (`500 detached_work`) |
| Service gave up | `usai_service{state="failed"} == 1` | the restart policy is exhausted; the instance stays ready, the service is down |
| Cron on two replicas | `sum(usai_scheduler{kind="cron"}) > 1` and the schedule is not `exclusive` | the schedule fires on each — declare it `exclusive: true` or start the others with `--no-cron` |
| **At the memory ceiling** | `increase(usai_process_memory_ceiling_hits_total[5m]) > 100` | the container is pinned at its limit and reclaiming the pages it is executing from — *not* an OOM kill, nothing is logged, and throughput collapses (measured: 1 req/s at p50 4.7 s in a box 16 MiB too small). Raise the limit or lower `--max-worlds` (`memory-pressure.md`) |
| Memory drift | `usai_process_proportional_memory_bytes` rising over hours while `usai_worlds_live` is flat and no revision was installed | the plateau is `base + touched slots × 4 MiB` (`memory-pressure.md`); a password-hashing burst or a held revision raises RSS for minutes, not hours — growth beyond that is a leak — report it with the soak samples |
| Dead letters | `increase(usai_queue_messages_total{state="dead"}[15m]) > 0` | messages out of attempts (`queue-dead-letter.md`) |
| Lost consumers | `increase(usai_queue_messages_total{state="reclaimed"}[15m]) > 0` | a consumer died mid-message (a crash, an OOM kill); the message was redelivered — look for the restart |
| 5xx rate | `rate(usai_http_responses_total{class="5xx"}[5m])` | as for any service; `deadline_exceeded` (504) is in it |
| Latency | `histogram_quantile(0.99, rate(usai_http_request_seconds_bucket[5m]))` | admitted requests only — read `usai_http_rejections_total` beside it |
