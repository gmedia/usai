# Supported envelope

What "supported" means here: inside this envelope the maintainers treat a
defect as a bug to fix, and the qualification campaigns (`docs/ROADMAP.md`
P5/P6) were run. Outside it Usai may well work, but nothing has been proven
and nothing is promised.

## Runtime

| | Supported | Notes |
|---|---|---|
| Linux x86_64 (glibc ≥ 2.36) | ✓ | Debian 12 / Ubuntu 22.04 and later; the Docker images are Debian bookworm |
| Linux aarch64 (glibc ≥ 2.36) | ✓ | same |
| macOS arm64 (14+) | development only | binaries are published; no production qualification |
| Windows | ✗ | use WSL2 or Docker |
| Kernel | ≥ 5.10 | the substrate uses `madvise`/`mprotect`-based memory reset; pagemap scanning is used when the kernel offers it |
| **Host envelope** | **192 MiB and 1 vCPU per application instance** (`--max-worlds 48`, PostgreSQL `pool.max` 4, status + metrics on) | measured 2026-09-19 (`docs/measurements/2026-09-20-p8e-efficiency.md`): idle 40 MiB RSS / 35 MiB PSS and 0.00 % CPU; a c=16 burst peaks at ≈102 MiB and 1 000 req/s on one vCPU, so 192 MiB leaves ≥ 25 % headroom. 128 MiB passes with 20 %; the technical floor is 48 MiB / 0.25 vCPU for a hello-only application (32 MiB is OOM-killed). Memory grows with the peak number of *concurrently used* worlds (≈4 MiB RSS / ≈1.5 MiB PSS per touched slot, kept resident so the next world does not fault the image back in), not with request count; size the limit for `40 MiB + max_worlds × 4 MiB` worst case, or lower `--max-worlds`. `USAI_WASM_KEEP_RESIDENT=0` returns the memory after a burst at ≈2× the CPU per request and half the throughput on one vCPU (measured; values between 0 and the default change nothing). A fractional vCPU works but its burst p99 is the CFS throttle (≈60–80 ms at 0.25–0.5 vCPU) |
| Many applications per host | ✓ one process per application | measured to N = 50 on one host: ≈30 MiB PSS (47 MiB RSS) and no idle CPU per idle application, linear in N; each holds its own `pool.max` connections (N × `pool.max` on the PostgreSQL side — 200 at N=50 with `pool.max` 4); a single process serving many applications is an open question (`docs/OPEN-QUESTIONS.md` Q17), not a feature |

## Data and network

| | Supported |
|---|---|
| PostgreSQL | 15, 16, 17, 18 (TLS with verified certificates; `sslmode=disable|prefer|require`) |
| Outbound HTTP | http/1.1 and h2 to any http(s) origin the application declares |
| Inbound HTTP | HTTP/1.1 behind a reverse proxy that terminates TLS (Caddy, nginx, an ingress) |
| WebSocket / SSE | ✓ through the same proxy; idle WebSockets are closed with 1008 after `USAI_SOCKET_IDLE_TIMEOUT` (300 s); qualified by the P6 `conn-churn` campaign (cycles, held through replacement/restart/proxy restart, abrupt client death, unread streams) |

## Deployment topology

| | Supported | Notes |
|---|---|---|
| One runtime instance behind a reverse proxy | ✓ | the topology the P5/P6 campaigns qualified (`docs/deploy/compose.production.yaml`) |
| N replicas behind a load balancer, one PostgreSQL | ✓ with the rules below | HTTP and queue consumers need nothing: consumers claim messages with `FOR UPDATE SKIP LOCKED`, so replicas share the work; migrations serialize on an advisory lock, so several `migrate` jobs at once apply each file exactly once |
| Cron on N replicas | **every instance that runs the scheduler ticks every schedule** | there is no leader election; run the scheduler on exactly one replica (`usai run --no-cron` / `USAI_NO_CRON=1` on the others) or write idempotent jobs. The topology does not change the application's semantics silently: each instance's `/_usai/status` says whether it schedules |
| `service()` on N replicas | **every instance that runs services has its own instance of each** | there is no leader election; a service that must be single (an ingest loop) runs on one replica (`usai run --no-services` / `USAI_NO_SERVICES=1` on the others) or is written for N (`for update skip locked`). One-shot commands (`usai app`, `db migrate`, …) never start services |
| Rolling deployment | ✓ | each instance drains its own in-flight work (`docs/runbooks/deploy-and-rollback.md`); connection-bound worlds are closed at the drain bound (WebSocket 1012, stream ended) and clients reconnect to the new instance |
| `cache.local` across replicas | ✗ | per process by definition; shared state belongs in PostgreSQL |

## Developer toolchain

| | Supported |
|---|---|
| Node | 24 (build toolchain, tests, `pnpm usai`) |
| pnpm | 12; npm and yarn work for installing, `pnpm` is what the scaffold and docs use |
| TypeScript | 5.9+ |
| Schemas | Zod 4 (Standard Schema + Standard JSON Schema); any Standard Schema library validates *in* the world only |

## Versioning and upgrades

- `0.0.x` (now): contracts may change between versions; each release note
  says what. Runtime and SDK are released together and must be at the same
  version; the runtime refuses an artifact of another manifest format before
  serving, with a message that names both versions.
- Upgrade path: build with the new SDK, deploy the new runtime with the new
  artifact. Rollback: activate the previous artifact on the previous runtime.
  The matrix runtime × artifact (N, N−1) is tested in CI on every push
  against the last published release.
- Migrations are immutable once applied (checksum-verified); a rollback of
  application code does not roll back the database — write migrations to be
  compatible with the previous code (add columns, do not drop them in the same
  release).

## Not supported (and not planned before 1.0)

HTTP/2 or TLS termination in the runtime, Redis/Kafka substrates, Node
compatibility (`require`, `process`, `fs` in a world), a hostile multi-tenant
sandbox (worlds are semantic isolation, ADR-0008), an ORM, multi-region.

## Reporting

Security issues: `SECURITY.md`. Everything else: an issue with `usai --version`,
the platform, and the smallest application that shows the problem.
