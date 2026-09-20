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
In-flight requests finish (bounded by the drain timeout, 30 s by default —
`--drain-timeout <s>` / `USAI_DRAIN_TIMEOUT`), new connections are refused by
the closed listener, services get their stop signal first, dispatched tasks
that were queued finish or are counted as `lost`. The orchestrator's grace
period must cover grace + drain timeout (the compose files use 35 s for
2 + 30). Measured: drain completes in < 1 s at 1 000 req/s. A single replica's
proxy answers 502 for the 2–3 s between exit and the replacement's first
listen — run two replicas and restart them one at a time
(`deploy-and-rollback.md` → Rolling restart): with the grace and a proxy that
retries a refused connection, the two-replica campaign measured **0 × 502 over
two restarts under load** (before the grace: one 502 per restart, a request
the proxy wrote onto an idle keep-alive connection the instant it closed).
Set the grace at or above the balancer's health-check interval.

A second SIGTERM/SIGINT forces the exit (`forced shutdown with N live
worlds and M live operations`, exit 130).

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
