# PostgreSQL down, restarted, or unreachable

## What you see

- Responses: **503** with `{"error":{"code":"pool_error" | "connection_closed", …}}`
  for every workload that leases the database; workloads without the
  resource keep answering 200. Refusals are fast: the campaign measured
  ~1 400 refusals/s at p99 < 15 ms — nothing queues behind the dead
  database.
- Metrics: `usai_http_responses_total{class="5xx"}` and
  `usai_http_workload_responses_total{workload,class="5xx"}` rise for the
  workloads that lease the database (the others stay 2xx);
  `usai_resource_quarantines_total{kind="postgres",name="main"}` rises by
  the number of pooled connections that were open when the server died —
  a counter, so alert on `increase()`; `usai_http_request_seconds` p99
  stays low.
- Log: `WARN connection quarantined: original query has no terminal outcome`
  and `application error … code="connection_closed"` for the connections
  that were open, then `code="pool_error" … cannot connect to <host:port> as
  user <user>: <reason>` for every attempt while the server is down (the
  same wording as the activation-time failure). Queue consumers log
  `claim failed; backing off`. `GET /_usai/ready` answers 503 with
  `resources: { main: "no connection: …" }` throughout.

## What the workloads do

Finite work fails terminally for that request (no implicit retry, ADR-0014):
the caller gets the 503 and decides. Queue messages are re-delivered when
the database returns (they were never claimed, or the claim's transaction
never committed). Cron ticks that fail are logged and skipped. Open
transactions are rolled back for the world (`rolledBackForWorld` in the
resource detail) and their connections quarantined when the rollback cannot
reach the server.

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
