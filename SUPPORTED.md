# Supported envelope

What "supported" means here: inside this envelope the maintainers treat a
defect as a bug to fix, and the qualification campaigns (`docs/ROADMAP.md`
P5/P6) were run. Outside it Usai may well work, but nothing has been proven
and nothing is promised.

## Runtime

| | Supported | Notes |
|---|---|---|
| Linux x86_64 (glibc ≥ 2.36) | ✓ | Debian 12, Ubuntu 24.04 and later; the Docker images are Debian bookworm. The published binaries are compiled on Debian 12 and the release refuses to publish one that asks for a newer glibc — 0.0.6 and earlier did ask (`pidfd_spawnp@GLIBC_2.39`) and start only on glibc ≥ 2.39 |
| Linux aarch64 (glibc ≥ 2.36) | ✓ | same |
| macOS arm64 (14+) | development only | binaries are published; no production qualification |
| Windows | ✗ | use WSL2 or Docker |
| Kernel | ≥ 5.10 | the substrate uses `madvise`/`mprotect`-based memory reset; pagemap scanning is used when the kernel offers it |
| **Host envelope** | **192 MiB and 1 vCPU per application instance** (`--max-worlds 48`, PostgreSQL `pool.max` 4, status + metrics on) | re-measured 2026-09-23 on a box **charged for its own page cache**, with the binary that ships (`docs/measurements/2026-09-23-floor-accounting.md`; the earlier figures were not charged and are void). The production shape holds **106 MiB resident / 93 MiB charged at its peak**, and the peak does not move with the box: 128, 192 and 256 MiB all measure the same, each serving ≈1 100 req/s at c=16 on one vCPU (p50 14 ms, 0 errors) and **touching the cgroup's ceiling zero times**. 192 MiB is the number to deploy on — it covers the ≈30 MiB a held revision costs during a rolling deployment, and a real application's per-slot working set is larger than the template's. 128 MiB serves this shape with no reclaim at all, but has no room for the held revision. The **technical floor** (hello only, `--max-worlds 16`) is **64 MiB**, *not* the 48 MiB published before: at 48 MiB the box is charged to its ceiling, reclaims the text it is executing 1.4 million times in one cell and serves **one request per second** — alive, never OOM-killed, and useless. Idle: ≈40 MiB RSS / 35 MiB PSS and 0.00 % CPU. Memory grows with the peak number of *concurrently used* worlds (≈4 MiB RSS / ≈1.5 MiB PSS per touched slot, kept resident so the next world does not fault the image back in), not with request count; size the limit for `40 MiB + max_worlds × 4 MiB + 30`, or lower `--max-worlds` — `usai run` warns when the limit is below that. `USAI_WASM_KEEP_RESIDENT=0` returns the memory after a burst at ≈2× the CPU per request and half the throughput on one vCPU. A fractional vCPU works but its burst p99 is the CFS throttle (≈60–80 ms at 0.25–0.5 vCPU). Watch `usai_process_memory_ceiling_hits_total` after any sizing change The image runs as **uid/gid 10001** (`usai`), needs no write access outside `/tmp`, and reserves ~200 GB of *virtual* address space per pool — `usai_process_virtual_memory_bytes` is not a leak and must not be capped (`LimitAS`, `ulimit -v`) |
| Many applications per host | ✓ one process per application | measured to N = 50 on one host: ≈30 MiB PSS (47 MiB RSS) and no idle CPU per idle application, linear in N; each holds its own `pool.max` connections (N × `pool.max` on the PostgreSQL side — 200 at N=50 with `pool.max` 4); a single process serving many applications is an open question (`docs/OPEN-QUESTIONS.md` Q17), not a feature |

## The npm compatibility budget

ADR-0013 said this had to be written down, and it had not been. What an
application may bring from npm, and what it may not:

| | |
|---|---|
| **Node** | 24 or newer, for the tooling — `usai build`, `usai dev`, `usai test` run Node; the application does not (it runs in a world). The published binaries need no Node at all to *serve* |
| **Package manager** | pnpm (what the scaffold and this repository use) or npm; both are tested — the scaffold's own CI uses npm |
| **TypeScript** | 5.9+, and only for your own typecheck: the build strips types with esbuild and never calls `tsc` except through your `tsconfig.json` |
| **The SDK's dependencies** | `@sakaladev/usai` depends on esbuild (the bundler) and nothing else at runtime; `zod` is yours to install and any Standard Schema library works |
| **What a world may import** | a package that only computes. Anything that reaches for a Node built-in when it is loaded cannot be bundled — the build says which package and why (GUIDE §7 → Query builders, and what you can npm-install) |
| **What it may not** | a driver, a connection pool, an ORM that opens its own connection, `fs`, `child_process`, native addons. The database, outbound HTTP, time and randomness are the runtime's to lease |

**The budget, stated plainly:** we will not polyfill Node globals to make a
package work, and we will not add an API without a lifetime rule (ADR-0013).
When a package that only computes cannot be bundled, that is a bug worth
reporting. When a package needs a socket, it is out of budget by design, and
the answer is a resource — an existing one, or a case for a new kind.

## Data and network

| | Supported |
|---|---|
| PostgreSQL | 15, 16, 17, 18 (TLS with verified certificates; `sslmode=disable|prefer|require`) |
| Outbound HTTP | http/1.1 and h2 to any http(s) origin the application declares |
| Inbound HTTP | HTTP/1.1 behind a reverse proxy that terminates TLS (Caddy, nginx, an ingress) |
| WebSocket / SSE | ✓ through the same proxy; idle WebSockets are closed with 1008 after `USAI_SOCKET_IDLE_TIMEOUT` (300 s); qualified by the P6 `conn-churn` campaign (cycles, held through replacement/restart/proxy restart, abrupt client death, unread streams) |
| Browser clients (SPA, mobile web) | ✓ cookie sessions (`auth.cookie`, `Secure; HttpOnly; SameSite=Lax` by default), bearer tokens as the second WebSocket subprotocol, `EventSource` resume via `last-event-id`; **CORS is not a runtime feature** — the proxy answers preflights, or the application declares `http.options("/*any")` and `defineApp({ headers })` (GUIDE §4, §9; THREAT-MODEL → browser posture) |

## Deployment topology

| | Supported | Notes |
|---|---|---|
| One runtime instance behind a reverse proxy | ✓ | the topology the P5/P6 campaigns qualified (`docs/deploy/compose.production.yaml`) |
| N replicas behind a load balancer, one PostgreSQL | ✓ with the rules below (campaigned with two: `docs/measurements/2026-09-18-p5-p6-qualification.md` → Two replicas) | HTTP and queue consumers need nothing: consumers claim messages with `FOR UPDATE SKIP LOCKED`, so replicas share the work (measured 497/503 of 1 000); a message a dead replica had claimed is reclaimed after the message deadline + 10 s (at-least-once: the consumer must tolerate the duplicate); migrations serialize on an advisory lock, so several `migrate` jobs at once apply each file exactly once. `/_usai/status` through the balancer is one replica's answer — read each replica's status listener |
| Cron on N replicas | **a schedule declared `exclusive: true` runs each tick on exactly one instance**; any other schedule ticks on every instance that runs the scheduler. **A tick with no scheduler alive is lost, not replayed** — and so is a tick that *failed*: the claim row stays, so no other replica reruns it — a rollout that stops the scheduling instance before starting the next one has no ticks in that window | `exclusive` claims the tick in PostgreSQL (`usai_cron_ticks`, one row per schedule and scheduled time — no leader, no election, a row insert); a schedule without it needs the scheduler on exactly one replica (`usai run --no-cron` / `USAI_NO_CRON=1` on the others) or idempotent jobs. The topology does not change the application's semantics silently: each instance's `/_usai/status` says whether it schedules and how many ticks another instance took |
| `service()` on N replicas | **every instance that runs services has its own instance of each** | there is no leader election; a service that must be single (an ingest loop) runs on one replica (`usai run --no-services` / `USAI_NO_SERVICES=1` on the others) or is written for N (`for update skip locked`). One-shot commands (`usai app`, `db migrate`, …) never start services |
| Rolling deployment | ✓ (measured: 0 × 502 over two restarts under load) | on SIGTERM an instance fails readiness and closes connections after their response for `--drain-grace` (2 s), then drains its in-flight work (`docs/runbooks/deploy-and-rollback.md` → Rolling restart, including what the proxy must do); connection-bound worlds are closed at the drain bound (WebSocket 1012, stream ended) and clients reconnect to the other instance |
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
  version; the runtime refuses an artifact of another **manifest format or
  guest ABI** before serving, with a message that names both versions.
  **That check is on the format, not on the version stamp**: an artifact
  built by a *newer* SDK than the runtime still *serves* whenever the format
  and the guest ABI did not change, which is the commonest rolling-deploy
  mistake — shipping the artifact before the binary. Since 0.0.10 it is not
  silent: the runtime logs `WARN this artifact was built by a newer SDK than
  the runtime serving it … deploy the runtime first`, naming both versions
  from the manifest's `builtWith`. It is a warning and not a refusal on
  purpose, because the artifact is usually fine and refusing would strand a
  rollback. Alert on that line.
- **`@sakaladev/create-usai` has its own version line** and is deliberately
  ahead (0.0.11 while the runtime and SDK were at 0.0.8): it is a scaffolder,
  not part of the runtime contract, and a fix to the template it writes does
  not wait for a release of the runtime. What it scaffolds is pinned to the
  runtime version (`^X.Y.Z` in the generated `package.json` and the matching
  images),
  so the version you check before a deployment is the one in *your* project,
  not the scaffolder's.
- **The SDK axis**, since 0.0.8: CI also builds an application against the
  *previous published SDK* and serves it on this runtime, or requires the
  refusal to name the guest ABI before anything listens.
- Upgrade path: build with the new SDK, deploy the new runtime with the new
  artifact. Rollback: activate the previous artifact on the previous runtime.
  The matrix runtime × artifact (N, N−1) is tested in CI on every push
  against the last published release.
- **The same artifact keeps the same identity across the upgrade.** An
  application's identity is a hash of its manifest as the runtime serializes
  it, plus the code hash, and a rolling deployment compares identities — so a
  runtime that adds a field when it reads an older manifest would turn one
  application into two mid-rollout. A unit test holds a real manifest from
  the previous SDK and asserts it serializes back unchanged; a new optional
  field has to be omitted when absent, and the test says so by name when it
  is not.
- Migrations are immutable once applied (checksum-verified —
  `usai db status` names a file edited after it was applied and exits
  non-zero, and `--check` also fails on anything pending). Each file is its
  own transaction, so keep an `ALTER TABLE` and a backfill in separate files
  and run a backfill as a `command()` rather than a migration; a file with
  `-- usai: no-transaction` runs outside the transaction, for `CREATE INDEX
  CONCURRENTLY` and its relatives. A rollback of application code does not
  roll back the database — write migrations to be compatible with the
  previous code (add columns, do not drop them in the same release).
  Changing a live schema end to end is
  [`docs/runbooks/schema-change.md`](docs/runbooks/schema-change.md).

## Not supported (and not planned before 1.0)

HTTP/2 or TLS termination in the runtime, Redis/Kafka substrates, Node
compatibility (`require`, `process`, `fs` in a world), a hostile multi-tenant
sandbox (worlds are semantic isolation, ADR-0008), an ORM, multi-region.

## Reporting

Security issues: `SECURITY.md`. Everything else: an issue with `usai --version`,
the platform, and the smallest application that shows the problem.
