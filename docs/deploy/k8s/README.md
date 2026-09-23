# Running Usai on Kubernetes

Two manifests, both annotated with what each value rests on:

- [`usai-app.yaml`](usai-app.yaml) — the web Deployment and everything around
  it: config, secrets, the migration Job, Services, a PodDisruptionBudget, a
  scrape target and the alerts from `../../runbooks/metrics.md`.
- [`usai-singletons.yaml`](usai-singletons.yaml) — the one-instance
  Deployment for schedules and services that must not run N times.

They were written by a platform engineer evaluating 0.0.9 from the public
documents alone, then reviewed here. Every number traces to a document or to
a measurement; where neither existed, the manifest says so rather than
pretending.

## The three decisions Kubernetes makes you take

### 1. Where the scheduler runs

`SUPPORTED.md` says to put `USAI_NO_CRON=1` on "all but one" replica. **A
Deployment cannot express that** — every pod gets the same PodSpec. Two
shapes work:

- **Declare the schedules `exclusive: true`** (GUIDE §6) and run one
  Deployment. Each tick is claimed once in PostgreSQL, whichever replica got
  there first, so an HPA can scale the Deployment freely. This is the shape
  to reach for.
- **Split**: a web Deployment with `USAI_NO_CRON=1 USAI_NO_SERVICES=1`, and a
  second Deployment with `replicas: 1` and `strategy: Recreate` that runs
  them. `Recreate` matters: a RollingUpdate with `maxSurge: 1` briefly runs
  two schedulers, which is exactly what the `sum(usai_scheduler{kind="cron"}) > 1`
  alert in `metrics.md` is for.

**A tick with no scheduler alive is lost, not replayed** (GUIDE §6), so the
`Recreate` gap has no ticks. If a job must survive a deploy, publish it to a
queue or let the handler work out what it missed from the data.

`service()` has the same shape and the same flag (`USAI_NO_SERVICES=1`).

### 2. When migrations run

`THREAT-MODEL.md` says to migrate as a deploy step rather than from the
serving process. On Kubernetes both of these are correct:

- a **`Job`** ordered before the rollout — a Helm `pre-upgrade` hook, an Argo
  `PreSync` wave, or a scripted `kubectl apply` + `wait`. A bare `Job` has no
  ordering relationship to a Deployment, so it needs one of those;
- an **initContainer on every replica**, which needs no ordering machinery at
  all: migrations serialize on an advisory lock and apply each file exactly
  once (measured with three concurrent migrators, `SUPPORTED.md`).

`usai-app.yaml` ships the Job and shows the initContainer commented beside
it. Pick one.

### 3. What a PodDisruptionBudget means during a database outage

A PDB counts **ready** pods, and by default a replica whose database probe
fails is unready (`postgres-down.md`). With one PostgreSQL behind every
replica, an outage makes them all unready at once, so `minAvailable: 1`
blocks **every voluntary eviction** — node drain, consolidation, cluster
upgrade — until the database comes back. That is defensible (evicting pods
during a database outage helps nobody) but it surprises people at 3 a.m.
`USAI_READY_REQUIRES_RESOURCES=0` is the other choice, and it is the same
trade-off the runbook describes for the proxy.

## What the runtime gives you, and what it does not

**Gives you**, measured on 0.0.9 (a developer box, not the qualification VM:
these are orderings and timings of events — a drain, a probe flip — not
throughput, and `docs/measurements/` is where performance numbers live):

- **A drain you can put a number on.** SIGTERM → exit is
  `USAI_DRAIN_GRACE + USAI_DRAIN_TIMEOUT` plus milliseconds, so
  `terminationGracePeriodSeconds` is that sum plus a margin. Readiness flips
  within ~16 ms of the signal while the listener keeps accepting, which is
  what a rolling update needs — and it is why these manifests carry **no
  `preStop` sleep hook**: the drain grace already is one.
- **Probes that mean different things.** `/_usai/ready` fails on drain and
  (by default) on a failing resource; `/_usai/live` stays 200 through both.
  Both stay open when `USAI_STATUS_TOKEN` is set, so the kubelet needs no
  credentials while Prometheus does.
- **Nothing under `/_usai/` on the application port** when the surfaces are
  on `--status-addr`, so the Service can expose 3000 without leaking them.

**Does not give you**:

- **A pod that waits for its database — unless you ask for it.** By default,
  a bound resource that cannot be reached at activation ends the process
  before any listener binds, which on Kubernetes is `CrashLoopBackOff` with a
  backoff that reaches five minutes: a pod that restarts for an unrelated
  reason during a database blip stays down *after* the database is healthy.
  **`USAI_ACTIVATION_RETRY=<seconds>`** keeps the process up and retrying
  instead, so the blip is absorbed by the `startupProbe` rather than by the
  restart backoff — set the probe's `failureThreshold × periodSeconds` above
  the retry budget, or the kubelet kills what the runtime is still waiting
  for. The manifests here set 90 s of retry against a 30 × 2 s startup probe
  (60 s), deliberately: the probe gives up first, and the pod restarts
  cleanly rather than sitting in a half-state. When the budget ends the
  process exits with the same error as before. A missing or malformed
  *variable* is never retried — that is a configuration error and fails at
  once. Whether retrying should be the default is
  `docs/OPEN-QUESTIONS.md` → Q20.
- **A "still booting" readiness answer.** The status listener binds after
  activation, so during startup a probe gets connection-refused rather than a
  503. Give `startupProbe` enough `failureThreshold` to cover a cold start
  (measured: ~130 ms warm, seconds on a cold image pull).

## Dashboards and alerts

`../grafana-dashboard.json` is a dashboard built from
`../../runbooks/metrics.md` — import it, pick the datasource and the job
label. The alert table on that page transcribes into a `PrometheusRule`
almost verbatim; the one alert that only exists here is
`usai_process_memory_ceiling_hits_total`, because a container reclaiming the
pages it executes from is invisible to Kubernetes: no OOM kill, no restart,
no event.

## Sizing

`../../runbooks/sizing.md` has the arithmetic; the short version for a
manifest:

```text
memory limit ≥ 40 MiB + peak_concurrency × (4…8 MiB) + 30 MiB   (a held revision)
memory floor  = 40 + 30 + max_worlds              (the runtime warns below it)
PostgreSQL    = replicas × pool.max, and a surge adds one replica's worth
```

The published envelope — 192 MiB, 1 vCPU, `--max-worlds 48`, `pool.max` 4 —
serves the production shape at ≈1 100 req/s with the kernel never reclaiming
(`docs/measurements/2026-09-23-floor-accounting.md`). These manifests ask for
more (320 MiB) because a Kubernetes limit is also the OOM boundary for a
burst, and because `requests` is what the scheduler packs on.

**Do not set an address-space limit.** The pool reserves ~200 GB of virtual
memory by design; `usai_process_virtual_memory_bytes` is not a leak and
`LimitAS`/`ulimit -v` kills the process at the first world.

## Autoscaling

CPU is the wrong first signal: the limiter in a real deployment is usually
the world budget or the connection pool, not the core. Scale on
`usai_world_budget{kind="in_use"} / usai_world_budget{kind="max"}` if you run
an adapter for custom metrics, and keep CPU as a secondary target. Whatever
you scale on, `maxReplicas × pool.max` (plus one surge replica) has to stay
under PostgreSQL's `max_connections`.
