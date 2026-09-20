# Bad deployment, rollback, revision replacement

## A broken artifact

`POST /revisions {"artifact": …}` on the control surface answers **422
`invalid_artifact`** with the reason (`this artifact uses manifest format 99
(built with SDK …, runtime …); runtime 0.0.4 understands format 1 only.
Rebuild …`), and nothing else happens: no revision is created, the active
one keeps serving, no log line beyond the request. The same holds for a
signature that does not verify (`artifact refused: …`) when the runtime
requires signatures. `usai run --artifact <bad>` exits 1 with the same
message before listening.

## Replacement without loss

Install (`revision installed`, ~70 ms with a precompiled image), activate
(`revision active`), then drain the previous one (`revision retired`). New
requests route to the new revision from the activation instant; in-flight
ones finish on the old one. The runtime's own test replaces revisions under
load and loses no request, and the campaign confirmed it through the proxy:
activate → rollback under 1 000 req/s, **0 failed requests, 0 bad seconds**
(log: `revision active rev2` / `revision draining rev1` … `revision active
rev1` / `revision draining rev2`).

## Rollback

Rollback is an install of the previous artifact plus an activation — the
runtime keeps no old revisions once drained. Keep the previous artifact
directory on the host (or the previous image tag). Measured: install +
activate + drain of the replaced revision in 6 s including the drain wait.

## Bounds

The runtime holds at most `max_revisions` (8) revisions across installed,
active and draining; an install past that answers **409
`too_many_revisions`** naming the fix (`DELETE /revisions/<id>` for what
will not be activated). Each held revision costs its compiled image (tens of
MB); the bound exists so an orchestrator bug hits it before the memory
limit does. `GET /revisions` lists them with state and in-flight count.

## Rolling restart (two or more replicas)

Restart one replica at a time and wait for its `/_usai/ready` to be 200
before the next. What each side must do, measured on the two-replica
campaign (`docs/measurements/2026-09-18-p5-p6-qualification.md` → Two
replicas):

- **The runtime** (on SIGTERM, before closing its listener): fails
  readiness (`503 {"reason":"draining"}`) and sends `Connection: close` on
  every response for `--drain-grace` seconds (default 2), then closes the
  listener and drains in-flight work. Set the grace at or above the
  balancer's health-check interval.
- **The proxy**: an active health check on `/_usai/ready` at an interval the
  grace covers, and a retry of a *refused* connection on another upstream
  (Caddy: `health_uri /_usai/ready`, `health_interval 1s`, `lb_try_duration
  5s`, `lb_try_interval 250ms` — `scripts/qualification/p5/Caddyfile.replicas`).
  A request already sent to a replica that dies is not retried (it may not
  be idempotent): a `kill -9` under load costs at most the requests in flight
  on that replica (measured: 1 of 40 543 in one run, 0 of 39 320 in another).
- **Connection-bound worlds** on the restarted replica end at the drain
  bound (WebSocket `1012 server draining`, event streams ended); clients
  reconnect and land on the other replica. Held connections on the other
  replica are untouched.

Result under 8 clients with one restart of each replica (the final run,
`2026-09-18-p5-p6-qualification.md` → Two replicas): 40 536 requests, 0 × 502,
0 errors, ready again 3–4 s after each restart.

## Status behind a load balancer

`/_usai/status` through the proxy is **one replica's** answer, and which one
alternates. Read each replica's status listener directly (`--status-addr`,
one port per replica) or scrape the metrics per instance.