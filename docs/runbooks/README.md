# Runbooks

One page per incident class, written from the P5 operational campaign on a
production-shaped deployment (Caddy → `usai run` in the runtime image →
PostgreSQL 18, 8 closed-loop load clients: ≈1 000 req/s in the 40 s failure
windows, ≈400 req/s averaged over the 24 h soak; evidence in
`docs/measurements/2026-09-18-p5-p6-qualification.md`). Each says
which metric moves, which log line appears, what the workloads do, and when
and how recovery happens — without reading Rust.

| Incident | Page |
|---|---|
| PostgreSQL down, restarted, or unreachable | [postgres-down.md](postgres-down.md) |
| Runtime stopped, killed, or restarted | [runtime-restart.md](runtime-restart.md) |
| Bad deployment, rollback, revision replacement | [deploy-and-rollback.md](deploy-and-rollback.md) |
| Overload and capacity refusals | [overload.md](overload.md) |
| **A route is slow but succeeding** (which route, waiting or computing, waiting on what) | [slow-route.md](slow-route.md) |
| Memory pressure / OOM | [memory-pressure.md](memory-pressure.md) |
| Invalid configuration at start | [invalid-config.md](invalid-config.md) |
| Disk full | [disk-full.md](disk-full.md) |
| A service gave up, `detached_work`, a 504, cron did not run, restart storms | [application-failures.md](application-failures.md) |
| Database backup, restore, connection limits, password and CA rotation | [postgres-operations.md](postgres-operations.md) |
| **Online schema change** (expand/contract, concurrent indexes, backfills, the mixed-version window) | [schema-change.md](schema-change.md) |
| A token or a signing key is compromised | [key-and-token-compromise.md](key-and-token-compromise.md) |
| Queue messages failing / dead letters | [queue-dead-letter.md](queue-dead-letter.md) |
| Every metric, its labels, and the alerts to set | [metrics.md](metrics.md) |
| **The log line**: its fields, what to label, what to leave alone, and the incident filter | [logs.md](logs.md) |
| Sizing: `--max-worlds`, `pool.max`, `concurrency`, memory, CPU, replicas | [sizing.md](sizing.md) |
| Running on a VM under systemd (deploy, rolling restart, rollback without Docker) | [systemd.md](systemd.md) |
| Running on Kubernetes (manifests, probes, the singleton scheduler, PDB × readiness) | [../deploy/k8s/README.md](../deploy/k8s/README.md) |
| Cutting a release (the three versions, the gate, the tag, what the workflow does) | [release.md](release.md) |

References the pages lean on: [`../ENVIRONMENT.md`](../ENVIRONMENT.md)
(every `USAI_*` variable) and [`../CONTROL-API.md`](../CONTROL-API.md) (the
orchestrator's surface).

Where to look, always:

- **`usai top --addr <status listener>`** first, on the box or from your
  laptop: everything below is *cumulative since the process started*, and an
  incident is about the last few seconds. It differences two samples —
  requests per second and the average and guest CPU per workload (waiting or
  computing, in one screen), rejections that never reached a workload, pool
  `in use` and `waiting`, and memory as the number that actually kills the
  process (the cgroup's charge against its limit, with RSS and PSS beside
  it, and a loud line when the box is reclaiming at its ceiling — which is
  how a too-small limit fails *without* an OOM kill). `-c 1` prints one
  screen to paste into a channel.
- `GET /_usai/status` (with `--status`): gauges (`liveWorlds`, `liveOps`,
  `detachedWorkDetected`), `revisions[]` (state, in-flight, services, queue
  counters), `resources[]` (`in_use`, `max`, `quarantined`, and per-kind
  `detail`), `http` (class counters, `rejections`, latency buckets),
  `process` (RSS, PSS, faults, CPU, threads, fds — and, when the process runs
  under a memory limit, that limit, what the cgroup is charged, and how often
  it has hit the ceiling).
- `GET /_usai/metrics`: the same facts as Prometheus text (the status document carries a few extra per-resource details; everything worth alerting on is in both)
  (`usai_http_request_seconds`, `usai_http_rejections_total{reason}`,
  `usai_resource{kind,name,metric}` (levels), `usai_resource_quarantines_total{kind,name}` (events), `usai_queue_messages_total`).
- Logs: one line per event at `info`; a request that reached a world and
  failed with a **5xx** logs `application error` with `code=`, `error=` (and a
  source-mapped stack) — *unless the 5xx is a lifecycle violation*
  (`detached_work` and friends), whose line is the teaching paragraph itself
  with the fact in `code=`; match those on `code`, never on the message — except a connection-level dependency failure, which
  is one `WARN dependency unavailable` per code per second with a
  `suppressed` count; 4xx answers (`not_found`, `conflict`, validation)
  are the application's answers, counted but not logged. The application's
  own `console.*`/`ctx.log.*` lines carry `target: "app"`. `RUST_LOG=debug`
  adds a `world trace` line per world. `--log-format json` for shipping.
- `GET /_usai/ready` / `GET /_usai/live` for orchestrators (readiness runs a
  bounded probe on every bound resource).
