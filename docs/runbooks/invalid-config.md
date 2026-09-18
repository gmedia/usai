# Invalid configuration at start

`usai run` reads the process environment only (never `.env`). A missing or
malformed declared variable fails **activation**, before anything listens:

```text
error: missing required environment: DATABASE_URL
  `usai run` reads the process environment only — it does not load .env … Export DATABASE_URL … and start again.
```

Exit code 1, no port bound, so an orchestrator's readiness check never
passes and the previous replica keeps serving. The same holds for a
resource that cannot start (`resource main failed to start: cannot connect
to …`, TLS that does not verify, an unknown `pool.recycling`) — all are
activation failures with the reason in the last line.

Through the control surface, activating a revision whose environment is
incomplete answers 422 `activation_failed` and leaves the current revision
active.
