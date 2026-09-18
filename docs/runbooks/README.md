# Runbooks

One page per incident class, written from the P5 operational campaign on a
production-shaped deployment (Caddy → `usai run` in the runtime image →
PostgreSQL 18, 8 load clients, ~1 100 req/s of real queries; evidence in
`docs/measurements/2026-09-18-p5-operational-qualification.md`). Each says
which metric moves, which log line appears, what the workloads do, and when
and how recovery happens — without reading Rust.

| Incident | Page |
|---|---|
| PostgreSQL down, restarted, or unreachable | [postgres-down.md](postgres-down.md) |
| Runtime stopped, killed, or restarted | [runtime-restart.md](runtime-restart.md) |
| Bad deployment, rollback, revision replacement | [deploy-and-rollback.md](deploy-and-rollback.md) |
| Overload and capacity refusals | [overload.md](overload.md) |
| Memory pressure / OOM | [memory-pressure.md](memory-pressure.md) |
| Invalid configuration at start | [invalid-config.md](invalid-config.md) |
| Disk full | [disk-full.md](disk-full.md) |
| Queue messages failing / dead letters | [queue-dead-letter.md](queue-dead-letter.md) |

Where to look, always:

- `GET /_usai/status` (with `--status`): gauges (`liveWorlds`, `liveOps`,
  `detachedWorkDetected`), `revisions[]` (state, in-flight, services, queue
  counters), `resources[]` (`in_use`, `max`, `quarantined`, and per-kind
  `detail`), `http` (class counters, `rejections`, latency buckets).
- `GET /_usai/metrics`: the same as Prometheus text
  (`usai_http_request_seconds`, `usai_http_rejections_total{reason}`,
  `usai_resource{kind,name,metric}`, `usai_queue_messages_total`).
- Logs: one line per event at `info`; every failed request that reached a
  world logs `application error` with `code=` (and a source-mapped stack).
  `RUST_LOG=debug` adds a `world trace` line per world.
