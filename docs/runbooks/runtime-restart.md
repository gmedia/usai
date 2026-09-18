# Runtime stopped, killed, or restarted

## SIGTERM (an orchestrator stop, `docker stop`, Ctrl-C)

Log, in order: `SIGTERM received` → `shutting down: draining in-flight work
(a second signal forces the exit)` → `http listener closed; draining
connections` → `revision retired` → `drained; ownership returned to
baseline`. Exit code 0. In-flight requests finish (bounded by the drain
timeout, 30 s), new connections are refused by the closed listener, services
get their stop signal first, dispatched tasks that were queued finish or are
counted as `lost`. Measured: drain completes in < 1 s at 1 000 req/s; the
proxy answers 502 for the 2–3 s between exit and the replacement's first
listen — run two replicas or a rolling restart to avoid that window.

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
