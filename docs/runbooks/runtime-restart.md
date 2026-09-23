# Runtime stopped, killed, or restarted

## SIGTERM (an orchestrator stop, `docker stop`, Ctrl-C)

Log: `SIGTERM received` → `shutting down: readiness now fails and
connections close after their response; the listener closes after the grace
period` (`--drain-grace <s>` / `USAI_DRAIN_GRACE`, default 2 s: `/_usai/ready`
answers 503 `draining` and every response carries `Connection: close` while
the listener still serves, so a balancer stops routing here and stops reusing
idle connections *before* they are refused) → `shutting down: draining
in-flight work (a second signal forces the exit)` → `revision retired` and
`http listener closed; draining connections` (either order; they are
concurrent) → `drained; ownership returned to baseline`. All are ordinary log
lines with timestamp and level (JSON with `--log-format json`). Exit code 0.

What happens to the work in flight, in order:

- **At the drain start** every connection-bound and background world of the
  revision gets the stop: WebSockets are closed with `1012 server draining`,
  a stream's and a task's `ctx.signal` aborts and a pending `ctx.sleep`
  returns, so a handler that checks the signal ends cleanly and a stream's
  body ends (the client reconnects to the replacement). Services get their
  stop first.
- **In-flight requests finish** (bounded by the drain timeout, 30 s by
  default — `--drain-timeout <s>` / `USAI_DRAIN_TIMEOUT`); new connections
  are refused by the closed listener; dispatched tasks that were queued
  finish or are counted as `lost`.
- **At the drain timeout** what has not returned is cancelled: `WARN drain
  did not finish; cancelling what is left` (`error="revision rev<n> did not drain
  within 30s"`), then `revision retired cancelled_in_flight=<n>`. A cancelled
  request's client gets a real response — `499` with
  `{"error":{"code":"cancelled"}}`, a status the runtime counts as
  client-side rather than a 5xx — not a dropped connection (measured on
  0.0.9: the client received body and status 8.0 s after SIGTERM with
  grace 2 and timeout 5); a cancelled queue message goes
  back for another attempt; a cancelled task logs `termination="cancelled:
  …"`. Nothing else is written on the way out.

The orchestrator's grace period must cover grace + drain timeout (the
compose files use 35 s for 2 + 30). Measured: drain completes in < 1 s at
1 000 req/s. A single replica's proxy answers 502 for the 2–3 s between exit
and the replacement's first listen — run two replicas and restart them one
at a time (`deploy-and-rollback.md` → Rolling restart): with the grace and a
proxy that retries a refused connection, the two-replica campaign measured
**0 × 502 over two restarts under load** (before the grace: one 502 per
restart, a request the proxy wrote onto an idle keep-alive connection the
instant it closed). Set the grace at or above the balancer's health-check
interval.

**Second and third signal.** A second SIGTERM/SIGINT *during the grace* ends
the grace early (`second signal: closing the listener now`) and the drain
proceeds normally. A second signal *during the drain* forces the exit
(`forced shutdown live_worlds=N live_ops=M`, exit 130) — in-flight requests
are lost exactly as under SIGKILL. So from a cold start: one signal is a
clean stop; two during the grace is a quick clean stop; two after the grace
(or three in total) is a forced exit.

## SIGKILL / crash

No log line (the process is gone). In-flight requests are lost (502 from
the proxy); the database is the source of truth, so nothing half-written
survives except what a committed transaction committed. Queue messages in
flight are re-delivered (at-least-once); in-memory dispatched tasks are
lost. Measured recovery: replacement container serving in 2 s.

Every restart logs `wasm engine …` then `precompiled application image
loaded … load_ms=70` then `revision installed` / `revision active`: an
artifact with `cache/image.cwasm` starts in well under a second. Without it
(`image rejected` or missing) the start compiles on every core for seconds.

## Restart storms

More than one `wasm engine` line per minute with no operator action is a
restart loop; the cause is in the lines just before each one:
`error: resource … failed to start` (dependency, see postgres-down),
`error: missing required environment` (invalid-config), or nothing at all
(OOM kill — see memory-pressure; `docker inspect` shows `OOMKilled`).
