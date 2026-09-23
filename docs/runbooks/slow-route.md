# A route is slow (but succeeding)

The incident every other page here does not cover: nothing is failing, no
alert is red, and a route that used to answer in milliseconds is taking
hundreds. Three questions, in this order — **which route**, **waiting or
computing**, and **waiting on what**.

## 1. Which route

**Not the global p99.** `usai_http_request_seconds` has no workload label, so
a slow route that is a minority of traffic does not move it: measured, a
route taking 204 ms left the p99 at **2.5 ms** because it was 98 requests
among 32 631. The alert on that histogram is for "everything got slower",
not for "one route did".

Use the per-workload mean instead — the numerator and denominator are both
labelled:

```promql
topk(5,
  rate(usai_http_workload_request_seconds_sum[5m])
/ rate(usai_http_workload_request_seconds_count[5m]))
```

Without Prometheus, the same numbers are in `/_usai/status`:

```bash
curl -s -H "authorization: Bearer $USAI_STATUS_TOKEN" localhost:9090/_usai/status \
| python3 -c 'import json,sys
h=json.load(sys.stdin)["http"]["by_workload"]
for w,c in sorted(h.items(), key=lambda kv: -(kv[1]["latencySumSeconds"]/max(kv[1]["count"],1))):
    if c["count"]: print(f"{1000*c[\"latencySumSeconds\"]/c[\"count\"]:8.1f} ms  {c[\"count\"]:>8}  {w}")'
```

A mean hides a bimodal route, but it finds the one to look at, which is the
step that was missing.

## 2. Waiting or computing

The answer decides everything after it — a slow query and a slow loop have
nothing in common — and two numbers give it.

**Per workload, from metrics**: CPU per request is
`rate(usai_workload_cpu_seconds_total[5m]) / rate(usai_http_workload_request_seconds_count[5m])`.
Compare it with the mean latency from step 1. Close together means the route
is **computing**; far apart means it is **waiting**.

**Per request, from the log**: the runtime's own trace line carries both.

```bash
RUST_LOG='usai_runtime::observability=debug' usai run --artifact … --log-format json
```

That target on its own is **one line per unit of work** — the whole
`RUST_LOG=debug` is seven lines per request and ~3.8 MB/s at four clients,
which is not something to turn on during an incident.

```json
{"message":"world trace","workload":"http:GET /invoices","request_id":"…",
 "duration_ms":206,"cpu_us":704,"termination":"completed","children":[]}
```

`duration_ms` is wall time, `cpu_us` is the world's own CPU:

| | `duration_ms` | `cpu_us` | reading |
|---|---|---|---|
| healthy | 1 | 1 273 | CPU ≈ wall: it computed, briefly |
| slow query | 206 | 704 | 0.3 % CPU — **waiting** |
| slow loop | 158 | 155 846 | 99 % CPU — **computing** |
| slow upstream | 502 | 653 | **waiting** |
| queued for a connection | 305 | 871 | **waiting** |

`request_id` joins that line to the application's own lines and to the
proxy's access log.

**Before you measure anything**, `/_usai/docs` (or
`/_usai/openapi.json` → `x-usai-resources`) already says which resources a
route can possibly wait on. A route that declares none and is slow is
computing — there is nothing else it can be.

## 3. Waiting on what

| what you see | what it is | what to do |
|---|---|---|
| `usai_resource{kind="postgres",metric="in_use"}` at `max`, `waiting` **0** | the pool is fully used and nothing is queued — the database is answering slowly | find the query (`pg_stat_statements`), add the index, or raise `pool.max` if the queries are as fast as they get |
| `usai_resource{metric="waiting"}` **> 0** | requests are queueing **for a connection**, not for the database | raise `pool.max`, lower the per-request work, or add replicas. At 10 s of waiting the operation is refused (`503 resource_exhausted`, `pool.acquireTimeoutSeconds`) |
| `usai_resource{kind="http.client",metric="in_use"}` at `max` | the outbound dependency is slow or down | `usai_resource_failures_total` says whether it is failing as well as slow |
| `usai_workload_cpu_seconds_total` climbing with latency | the application's own code | it is in the handler; `console.time` does not exist in a world, so bracket the phases with `Date.now()` and `ctx.log.info` |
| Nothing moves, latency is flat, one client is slow | not the server | the proxy, the client, the network — `x-request-id` joins the proxy's log to the runtime's |

A note that has cost people time: `in_use == max` is **healthy saturation**
on its own. The alert in `metrics.md` that fires on it is worth having, but
read `waiting` beside it before changing anything.

## Measuring one request

`--diagnostics` adds `x-usai-server-ms` to every response, which is the
runtime's own view of that request. It also exposes error details and stacks
to clients, so it belongs on a trusted network or a test environment, not in
front of users.

`USAI_PROFILE=1` adds the per-phase ledger (`x-usai-profile`) — routing,
decoding, validation, admission, the world, the guest's own phases. It
allocates per request; it is for a measurement session, not for production.

## What this page cannot tell you

- **Which SQL statement is slow.** That is PostgreSQL's own instrumentation
  (`pg_stat_statements`, `auto_explain`); the runtime knows a query took
  200 ms, not what the planner did.
- **Where in your JavaScript the time went.** There is no sampling profiler
  in a world. `Date.now()` around the phases and a `ctx.log.info` with the
  numbers is the honest answer, and it lands at the default log level with
  the workload and the request id already attached.
