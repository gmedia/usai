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

Without Prometheus, `usai top` is the same numbers on the box — and over
the **last few seconds** rather than since the process booted, which is what
an incident is asking about:

```console
$ usai top --addr 127.0.0.1:9090
checkout  revision 7 active  engine wasm  every 2.0s

  worlds 12/256 live, 12 in flight   tasks 0 running, 0 queued (max 64)
  mem 121.4/192 MiB charged (rss 155.0, pss 118.2)   cpu 41%   fds 37   threads 20   worlds/s 216

  workload                        req/s   live    4xx    5xx        avg     cpu%
  http:GET /catalog               201.5      3      0      0     2.10ms      38%
  http:POST /orders                14.2      9      0      0      204ms      11%
  stream:GET /exports.csv           0.0      2      0      0          —      24%

  resource                 in use  waiting      ops/s      state
  db (postgres)             16/16        9      118.0      ready
```

`-n <seconds>` sets the window, `-c 1` prints one screen and exits (the form
to paste into an incident channel), `--status-token` when the instance needs
one. The **`live`** column is worlds running right now, and it is the one to
read for anything connection-bound: a stream or a socket completes no
requests while it runs, so it moves no rate — the row above shows two
exports in flight at `0.0` req/s, and a workload that has finished nothing
at all still gets a row when it has a world alive. **`cpu%` is a share of one
core over the window**, not milliseconds per request, for the same reason: a
per-request figure divides to zero for work that has not finished, and the
workload eating the box would read as free. `avg` is `—` when nothing
completed in the window, because there is no average to report. A mean hides a bimodal route, but it finds the one to look at, which is
the step that was missing — and the `avg` and `cpu%` columns beside each other
are step 2 below, already answered: a route whose `cpu%` accounts for most of
its `avg` is computing, one whose does not is waiting.

## 2. Waiting or computing

The answer decides everything after it — a slow query and a slow loop have
nothing in common — and two numbers give it.

**Per workload, from metrics**: CPU per request is
`rate(usai_workload_cpu_seconds_total[5m]) / rate(usai_http_workload_request_seconds_count[5m])`.
Compare it with the mean latency from step 1. Close together means the route
is **computing**; far apart means it is **waiting**.

**Per request, from the log**: the runtime's own trace line carries both.

```bash
RUST_LOG='warn,usai_runtime::observability=debug' usai run --artifact … --log-format json
# the leading `warn,` matters: the filter replaces the default, so without it
# you lose every `application error` and `task failed` line you came for
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
| `rate(usai_resource_operation_seconds_total)` divided by `rate(usai_http_workload_request_seconds_sum)` for the route | **how much of this route's time is spent inside that resource** — 0.9 says the request is the database, 0.1 says it is somewhere else | this is the number that decides whether to go to `pg_stat_statements` at all. Divide the same counter by `usai_resource_operations_total` for the mean time per operation |
| `usai_resource{metric="waiting"}` **> 0** | requests are queueing **for a connection**, not for the database | raise `pool.max`, lower the per-request work, or add replicas. At 10 s of waiting the operation is refused (`503 resource_exhausted`, `pool.acquireTimeoutSeconds`) |
| `usai_resource{kind="http.client",metric="in_use"}` at `max` | the outbound dependency is slow or down | `usai_resource_failures_total` says whether it is failing as well as slow |
| `usai_workload_cpu_seconds_total` climbing with latency | the application's own code | it is in the handler; bracket the phases with `console.time("label")` / `console.timeEnd("label")`, which log at INFO with the workload and the request id |
| Nothing moves, latency is flat, one client is slow | not the server | the proxy, the client, the network — `x-request-id` joins the proxy's log to the runtime's |

A note that has cost people time: `in_use == max` is **healthy saturation**
on its own. The alert in `metrics.md` that fires on it is worth having, but
read `waiting` beside it before changing anything.

## Measuring one request

`--server-timing` (`USAI_SERVER_TIMING=1`) puts the standard header on every
response the application produced:

```
Server-Timing: total;dur=214.208, world;dur=213.006, cpu;dur=8.412
```

`total` is the request end to end, `world` is how long the world was alive
(the difference is routing, decoding, validation and encoding), and `cpu` is
how much of the world was the guest **computing** rather than waiting on a
resource — a `cpu` far below `world` means the handler is waiting on the
database or an upstream, and a `cpu` close to it means the handler itself is
the cost. It is timing and nothing else — no stacks, no error text — so it
may stay on in production; it is off by default because it does tell a
client something about the server. `curl -D- -o/dev/null`, a browser's
network panel and most proxies read it without any setup. A request rejected
**before** a world exists (an unknown route, a schema failure, a refused
admission) carries no header at all, which is itself the answer: nothing of
the application ran.

`--diagnostics` adds `x-usai-server-ms` to every response, which is the
runtime's own view of that request. It also exposes error details and stacks
to clients, so it belongs on a trusted network or a test environment, not in
front of users. `--server-timing` is the part of it that is safe to keep.

`USAI_PROFILE=1` adds the per-phase ledger (`x-usai-profile`) — routing,
decoding, validation, admission, the world, the guest's own phases. It
allocates per request; it is for a measurement session, not for production.

## What this page cannot tell you

- **Which SQL statement is slow.** That is PostgreSQL's own instrumentation
  (`pg_stat_statements`, `auto_explain`); the runtime knows the resource's
  operations took 200 ms in total (`usai_resource_operation_seconds_total`),
  not which statement or what the planner did.
**Streams.** A stream's latency is its **world's lifetime**, recorded when
the world ends — so a six-second export is six seconds in
`usai_http_workload_request_seconds_sum` and in `usai top`'s `avg`, and it
arrives a moment after the body does. Its `Server-Timing` header says
`head;dur=…` rather than `total`, because the header leaves with the head
and there is no honest total to put in it.

- **Where in your JavaScript the time went.** There is no sampling profiler
  in a world. `console.time("query")` / `console.timeEnd("query")` around the
  phases is the honest answer, and it lands at the default log level with the
  workload and the request id already attached.
