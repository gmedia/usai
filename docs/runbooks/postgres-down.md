# PostgreSQL down, restarted, or unreachable

## What you see

- Responses: **503** with `{"error":{"code":"pool_error" | "sql_57p01" |
  "connection_closed", …}}` for every workload that leases the database;
  workloads without the resource keep answering 200 **from the runtime** —
  see the box below before you believe that through your proxy. Refusals are fast: the
  campaign measured ~1 400 refusals/s at p99 < 15 ms — nothing queues behind
  the dead database. Which code depends on where the request caught the
  outage: `sql_57p01` (`terminating connection due to administrator
  command`) or `sql_57p03` for a query that was on the wire when the server
  went down cleanly, `connection_closed` for one whose connection vanished
  without a word (a kill −9, a network partition), `pool_error` for every attempt while
  the server is unreachable. **That code is what the client sees; the
  wording is what the log sees** — a client's body is
  `{"error":{"code":"pool_error","message":"internal error"}}`, while the
  log line carries `cannot connect to <host:port> as user <user>: <reason>`,
  the same wording as the activation-time failure. The host and the user are
  operator information and do not leave the process. Clients see the status and the code; they do not need to tell
  them apart.
> **Through a proxy that health-checks `/_usai/ready`, this is a total
> outage, not a partial one.** Readiness fails when any bound resource fails
> its probe, a proxy removes an unready upstream from rotation, and every
> replica shares the database — so the proxy runs out of upstreams and
> answers 503 for *every* route, including the ones that never touch
> PostgreSQL. Observed in the 2026-09-23 operator round on a two-replica
> systemd deployment: direct to a replica `hello` answered 200 and the
> database-backed route 503, while through the proxy at the same moment both
> answered 503 (`docs/STATUS.md` → the tenth round). This is
> the default because a replica that cannot reach its database cannot serve
> most of the application, and because it stops a rollout from going live
> broken. If you would rather keep resource-free routes answering, set
> `USAI_READY_REQUIRES_RESOURCES=0` on every replica: `/_usai/ready` then
> answers 200 and still names the failing resource in `resources`, so the
> alert below is unchanged, and draining still fails readiness (the rolling
> restart is unaffected). Decide this before the outage, not during it.

- `/_usai/status` → `resources[].ready` is **false** from the first
  connection-level failure (with `detail.lastError` and
  `detail.unreadyForSeconds`) and true again after the first successful
  query or probe. The probe is bounded inside the resource, so a database
  that has stopped answering at all — the usual shape of an unreachable
  host — flips `ready` within a second of the first failed probe, without
  waiting for a real query to come along (it did wait, before 0.0.9); `GET /_usai/ready` answers 503 with
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

## Refused versus unreachable

Two outages look different on the wire. A **stopped server** refuses the
connection at once (`connection refused`): every attempt fails in
microseconds, the 503s are fast, worlds are held for nothing. An
**unreachable server** — a network partition, a firewall dropping SYNs, a
host that is gone — answers nothing, and every attempt waits the connect
timeout: **5 s by default**, so every request that needs the database holds
its world for 5 s before its 503. At 1 000 req/s that is 5 000 worlds
wanted against a `--max-worlds` of 256: the instance answers `503
capacity_exhausted` for most of them until the network returns. That is
the admission bound doing its job (`overload.md`), but the recovery is
slower and the worlds are wasted. Set the connect timeout in the URL for a
service that must fail fast — `postgres://…/app?connect_timeout=2` (seconds,
PostgreSQL's own parameter; the runtime applies 5 when it is absent) — and
keep it above the network's real connect latency.

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
