# Roadmap to production-ready

> The phase decides what work is allowed. From here on the work is
> **qualification, not capability**: every phase removes reasons a developer
> or an operator still has to distrust Usai. Features are added only when a
> phase proves a real application cannot be built without them.

Current phase: see `STATUS.md`. Versions stay `0.0.x` until RC; breaking
changes are allowed and preferred over carrying a wrong abstraction.

| Phase | Focus | Exit criteria |
|---|---|---|
| **P3** | Dogfood fixes | The fresh-eyes report's contract/correctness items closed; a fresh scaffold installs, typechecks, tests and runs without friction. **Done in 0.0.2** (open: `fetch`/`crypto` as host capabilities — a P4 decision). |
| **P3.5** (done 2026-09-18: v0.0.4) | Distribution & container UX | Native binary, npm packages and Docker images (`sakaladev/usai:<v>` runtime, `<v>-dev` builder) from **one tag**; linux/amd64 + linux/arm64; `docker compose up` path in the scaffold; runtime image non-root, no toolchain, read-only artifact, graceful SIGTERM; artifact↔runtime compatibility refused deterministically before serving; container smoke test in CI. Docker is a zero-install path, never the only path. |
| **P4** (done 2026-09-18: `examples/invoicing`, ADR-0017) | Real application | One realistic application — auth, tenants, CRUD with transactions and pagination, validation, errors, background task, cron, queue, outbound HTTP, migrations, seeders — built and used as a user, not as the author, without touching runtime internals. If a fundamental primitive is missing, that is the finding (outbound HTTP becomes an ownership-aware host operation here, not a polyfill). Boring is the goal. |
| **P5** (done 2026-09-18: runbooks, campaign) | Operational qualification | Deployed for real (reverse proxy → Usai → PostgreSQL) for days; deliberately broken: runtime restart, PostgreSQL kill/restart, network interruption, bad deployment, rollback, rapid revision replacement, traffic spike, memory pressure, near-full disk, SIGTERM, forced kill, invalid config. A runbook per incident: which metric moves, which log line appears, what workloads do, when recovery happens — without reading Rust. |
| **P6** (campaigns done 2026-09-18; WebSocket/stream campaign written 2026-09-19 and chained after the 24 h soak; 72 h soak chained after it) | Reliability qualification | 24 h and 72 h realistic soaks with a memory plateau and zero ownership/state leak; revision churn (deploy A, B, rollback A, C, broken D refused, E …) thousands of times under traffic; DB flap, idle→burst, overload, shutdown/restart loops, queue retry/dead-letter, WebSocket/stream churn, artifact upgrade/rollback. Repeated, not passed once. |
| **P7** | External validation | 3–10 developers with no context build and deploy a small API from the public docs alone. Measured: time to hello world, time to PostgreSQL, what was misunderstood, which API needed the source, which errors did not help, whether the lifecycle model felt natural. Five people making the same mistake means the API is wrong. |
| **RC / Beta** | Freeze-ish contracts | `http.*`, `task`/`cron`/`command`/`service`/`queue`, resources, env, config, artifact format, CLI stable; upgrade/downgrade matrix (runtime × artifact × SDK) regression-tested; `SUPPORTED.md` written. |
| **Production Ready** | Every gate green | Automated evidence **and** real operational evidence **and** external developer evidence. Also: artifact signing/attestation verified before activation (`cache/image.cwasm` is native code); observability round two (latency histogram, error rate, admission rejections, live worlds, task/queue state, pool/quarantine, revision state, CPU/memory pressure); the vendored Wasmtime patch upstreamed or with documented provenance and a rebase test; security qualification inside the supported envelope (fuzzing of contracts, artifacts and the control API; dependency scanning; secret-leak checks; body/timeout/memory limits). Only then comparative benchmarks (Node, PHP, Bun, Deno, Rust baseline) with the same workload — reported as they come out. |

## Not blockers for the first production-ready

HTTP/2, built-in HTTP TLS, Redis, Kafka, Node compatibility, a hostile
multi-tenant sandbox, an ORM, multi-region, every Web API. Production-ready
means: *within the environment we declare supported, Usai can be run,
upgraded, loaded, failed, recovered and observed with proven behaviour* — not
feature parity with anything.

## Distribution paths (P3.5 onward)

```text
1. native binary        usai dev               (first-class; releases page)
2. Docker               docker compose up      (zero-install, not zero-cost: bind mounts on macOS/Windows)
3. npm-assisted binary  pnpm usai dev          (the SDK's `usai` bin fetches the release binary at its version, once)
```

Three artefacts from one tag `vX.Y.Z`: GitHub release binaries, npm
`@sakaladev/usai` + `@sakaladev/create-usai`, Docker `sakaladev/usai:X.Y.Z`
(+ `X.Y`, `X`, `latest`) and `X.Y.Z-dev`. No version skew between them.
