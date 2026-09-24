# Bad deployment, rollback, revision replacement

The endpoints, their bodies and every error code are in
`docs/CONTROL-API.md`; this page is what happens when you use them.

## Rolling back is two axes

The artifact and the binary move independently, and this page is about the
first. The second is `SUPPORTED.md` → Versioning: the supported pair is a
runtime with an artifact built by the **same or the previous** version, so a
version rollback is *activate the previous artifact, then run the previous
binary* — in that order, because the old binary may not understand the new
artifact.

Two things make it safe, and both were verified for this release:

- **The same artifact keeps the same identity across a binary swap**, so a
  rolling deployment comparing identities sees one application in both
  directions rather than two.
- **A failed build leaves the previous artifact servable**, so the thing you
  roll back to is still there.

And one thing to watch: rolling back only the **binary** leaves the new
artifact on the old runtime. That is the unsupported half of the matrix — it
may work, nothing tests it, and the refusal that would tell you is on the
manifest format and the guest ABI, which do not move every release. Roll both
back or neither.

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
  (the block is below). A request already sent to a replica that dies is not
  retried (it may not be idempotent): a `kill -9` under load costs at most
  the requests in flight on that replica (measured: 1 of 40 543 in one run,
  0 of 39 320 in another).

### The proxy configuration, in full

This is the configuration the 0 × 502 result was measured with. Copy it;
it is not an illustration.

```caddyfile
:8080 {
	reverse_proxy 127.0.0.1:3001 127.0.0.2:3001 127.0.0.3:3001 {
		lb_policy round_robin
		# A draining replica answers 503 on /_usai/ready within a second of
		# the stop signal, so the check takes it out before its listener
		# closes; a connection that is refused (the replica is restarting)
		# is retried on another upstream instead of becoming a 502.
		health_uri /_usai/ready
		health_port 9090
		health_interval 1s
		health_timeout 2s
		fail_duration 5s
		lb_try_duration 5s
		lb_try_interval 250ms
		transport http {
			dial_timeout 2s
			response_header_timeout 30s
		}
	}
}
```

**`health_port` is one value for the whole handler.** It is the single
mistake this block exists to prevent: with replicas on `127.0.0.1:9091`,
`127.0.0.1:9092`, … there is no way to write it, and the obvious
`health_port 9091` health-checks *every* replica on replica 1's status
port — so replica 1 going down removes replicas 2 and 3 from rotation
too. It passes `caddy validate` and it was measured producing 503s while a
perfectly healthy replica sat idle behind it.

Give each replica its **own loopback address and the same ports** instead
(`127.0.0.1`, `127.0.0.2`, `127.0.0.3` — all of `127.0.0.0/8` is local on
Linux, no configuration needed):

```ini
# usai-app@.service, replica %i on 127.0.0.%i
Environment=USAI_STATUS_ADDR=127.0.0.%i:9090
ExecStart=/usr/local/bin/usai run --artifact /srv/app/current --host 127.0.0.%i --port 3001
```

The same shape works for any proxy whose health check is per-upstream
rather than per-handler (nginx `upstream` with `health_check port=`,
HAProxy `server … check port 9090`, Envoy per-endpoint health checks); with
those, one status port per replica is fine and the loopback trick is not
needed. In a container topology each replica is its own host name and the
problem does not arise (`scripts/qualification/p5/Caddyfile.replicas` is the
compose form: `reverse_proxy app:3000 app2:3000 app3:3000`).

**Health-checking `/_usai/ready` couples every route's availability to
every bound resource.** A replica whose database probe fails is *unready*,
and a proxy removes an unready upstream from rotation — including for
routes that never touch the database. When the database is shared, every
replica fails at once and the proxy has nothing left to send to: a
PostgreSQL blip becomes a total 503, not a partial one. That is the
default because a replica that cannot reach the database really cannot
serve most of an application, and because it is the behaviour that makes a
rollout stop instead of going live broken. If you would rather keep
resource-free routes serving through a database outage, set
`USAI_READY_REQUIRES_RESOURCES=0` on every replica: readiness then reports
resource health without failing on it, drains still fail readiness (the
rolling restart above is unaffected), and `postgres-down.md` describes what
each choice looks like at 3 a.m.
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