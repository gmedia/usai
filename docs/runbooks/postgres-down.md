# PostgreSQL down, restarted, or unreachable

## What you see

- Responses: **503** with `{"error":{"code":"pool_error" | "sql_57p01" |
  "connection_closed", …}}` for every workload that leases the database;
  workloads without the resource keep answering 200. Refusals are fast: the
  campaign measured ~1 400 refusals/s at p99 < 15 ms — nothing queues behind
  the dead database. Which code depends on where the request caught the
  outage: `sql_57p01` (`terminating connection due to administrator
  command`) or `sql_57p03` for a query that was on the wire when the server
  went down cleanly, `connection_closed` for one whose connection vanished
  without a word (a kill −9, a network partition), `pool_error … cannot
  connect to <host:port> as user <user>: <reason>` for every attempt while
  the server is unreachable — the same wording as the activation-time
  failure. Clients see the status and the code; they do not need to tell
  them apart.
- `/_usai/status` → `resources[].ready` is **false** from the first
  connection-level failure (with `detail.lastError` and
  `detail.unreadyForSeconds`) and true again after the first successful
  query or probe; `GET /_usai/ready` answers 503 with
  `resources: { main: "no connection: …" }` throughout.
- Metrics: `usai_http_responses_total{class="5xx"}` and
  `usai_http_workload_responses_total{workload,class="5xx"}` rise for the
  workloads that lease the database (the others stay 2xx);
  `usai_resource_quarantines_total{kind="postgres",name="main"}` rises by
  the number of connections that had a query **in flight** when the server
  died — a counter, so alert on `increase()`. Idle pooled connections are
  not quarantined: they are simply gone (the pool notices on the next lease
  and opens a new one), so an outage at low traffic may show `0`
  quarantines. `usai_http_request_seconds` p99 stays low (refusals are
  counted before a world exists and are not in the histogram; the 503s that
  do reach a world are fast).
- Log: for the connections that had a query in flight, `WARN connection
  quarantined: original query has no terminal outcome`; then **one line per
  second**, `WARN dependency unavailable code="pool_error" … suppressed=<n>`
  (`suppressed` is how many requests since the previous line failed the same
  way — the runtime does not write a stack per request while a dependency
  is down). Queue consumers log `claim failed; backing off`; when the
  database answers again, `INFO database reachable again`.

## What the workloads do

Finite work fails terminally for that request (no implicit retry, ADR-0014):
the caller gets the 503 and decides. Queue messages are re-delivered when
the database returns (they were never claimed, or the claim's transaction
never committed). Cron ticks that fail are logged and skipped. Open
transactions are rolled back for the world (`rolledBackForWorld` in the
resource detail) and their connections quarantined when the rollback cannot
reach the server. Quarantine is about *proof*, not about the outage: a
connection is quarantined when a query on it has no terminal outcome (C5),
never merely because the server went away while it sat idle.

## Recovery

Automatic. The pool creates connections lazily; the first request after
`pg_isready` succeeds. Measured: kill −9 → PostgreSQL back → **first 200
within 1 s**, total impact = PostgreSQL's own restart time (8–12 s here);
`docker restart postgres` → 1 second of 503s; a 10 s network partition →
1 second of 503s after the network returned. No runtime restart is needed.

Nothing to do unless `quarantined` keeps rising *after* the database is
back: that means connections keep dying (a proxy or firewall idle timeout,
a wrong `sslmode`) — check the `reason` in the `cannot connect` line.

## Prevention

Use `sslmode=require` and a `caFile` so a wrong endpoint fails activation,
not requests. Size `pool.max` below PostgreSQL's `max_connections` minus
what other clients use; `usai_resource{metric="in_use"}` at `max` with a
rising `503 resource_exhausted` count means the pool, not the server, is
the limit.
