# Changelog

User-visible changes per release, newest first. Runtime, SDK and CLI ship
together at the same version; an artifact built by one version runs on the
runtime of that version and the next (`SUPPORTED.md` → Versioning). Within
`0.0.x` contracts may change between versions; each entry says what.

GitHub releases carry the auto-generated commit list as well; this file is
the human summary.

## 0.0.6 — unreleased

Since 0.0.5 (2026-09-19). Alpha, production qualification in progress: the
24 h soak, the failure campaigns, the connection campaign and the
two-replica campaign passed; the 72 h soak is running.

### Runtime

- **Hot path halved at c=1** (P8, `docs/measurements/2026-09-19-p8-parity.md`):
  hello 1.70 → 0.90 ms p50, contract-heavy 5.27 → 1.28 ms. Four levers, no
  contract weakened: the snapshot warms the validators' accepting path; the
  guest is entered by direct calls, not `eval` (ADR-0018 — `GUEST_ABI` stays 1,
  0.0.5 artifacts still run); a boundary slot whose schema is proven final is
  validated once (host), the world only strips undeclared keys; owned buffers
  and an in-core settle make a request four typed calls.
- **Idle cost is zero.** The watchdog and the epoch ticker park while no world
  is live (an idle instance woke 300 times a second before). Fifty idle
  instances on one host: 0.02 % of a core.
- **Operator surfaces can be protected and switched off.** `USAI_STATUS_TOKEN`
  (`--status-token`): status, metrics and `openapi.json` answer 401 without
  `Authorization: Bearer`; probes and the docs shell stay open (the shell asks
  for the token). `USAI_SURFACES_OFF=status,metrics,docs,live,ready`
  (`--surfaces-off`): a switched-off surface is a 404.
- **`/_usai/status` reports the process** (`process`: RSS, PSS, virtual size,
  peak RSS, page faults, CPU seconds, threads, fds) and the metrics gain
  `usai_process_*`. Counters are named: `rejections` by reason, per-workload
  `{2xx,3xx,4xx,5xx}`, latency buckets as `{le, count}`; `scheduler`
  says whether this instance runs cron, queue consumers and services.
- **Replica flags.** `usai run --no-cron` / `--no-queue` / `--no-services`
  (`USAI_NO_CRON`, `USAI_NO_QUEUE`, `USAI_NO_SERVICES`): cron ticks on every
  instance that schedules, services run on every instance that runs them —
  keep each on the replicas you mean. One-shot commands (`usai app`, `task run`,
  `db migrate|status|seed`) no longer start services or queue consumers (a
  `db migrate` job used to run the application's `service()` loops).
- **Migrations serialize on an advisory lock**: N concurrent `migrate` jobs
  apply each file exactly once; the queue table's creation no longer races.
- **A queue message claimed by a consumer that died is reclaimed.** A replica
  killed mid-message (SIGKILL, OOM) used to leave its rows `processing`
  forever. One sweeper per topic puts such a row back for another attempt when
  the declared retry policy has one left, or dead-letters it with `consumer
  lost: claimed by <who> at <when>, never completed`; `/_usai/status` counts
  them (`queue.reclaimed`).
- **Rolling restarts without a 502.** `usai run --drain-grace <s>`
  (`USAI_DRAIN_GRACE`, default 2): on SIGTERM the listener stays open while
  `/_usai/ready` answers 503 `draining` and every response carries
  `Connection: close`, so a load balancer stops routing here and stops reusing
  idle connections before the listener closes; then the drain proceeds. A
  second signal skips the grace. `usai dev` and the test harness use 0.
- **WebSocket authentication happens before the 101.** A failing auth
  resolver answers 401 as any request would (before: a 101 followed by a bare
  close frame). Browsers pass the token as the second subprotocol
  (`new WebSocket(url, ["bearer", token])`); the runtime echoes
  `Sec-WebSocket-Protocol: bearer`. A 101 is counted as an upgrade, not a 5xx;
  `upgrades` and `streams` counters move.
- **HTTP boundary**: a non-JSON body on a JSON contract is `415
  unsupported_media_type`; `405` carries `Allow`; a 500 whose code is not one of
  the runtime's own (`sql_22003`, a parameter position) is `internal` to the
  client outside `--diagnostics`, the code stays in the log; event streams carry
  `Cache-Control: no-cache` and `X-Accel-Buffering: no`; a detached write
  (`ctx.resources.db.execute(...)` without `await`) fails the request with `500
  detached_work` instead of committing a response the write did not reach.
- **Operations answer by name**: a handler that cannot start an operation gets
  `unknown_task`, `capacity_exhausted` (503) or `unknown_operation` instead of
  a bare `op_refused`.
- `publishes()` to a topic no consumer consumes is a WARN at activation and
  marked in `inspect`; a module migration/seeder glob matching no file is a
  WARN; a service that exhausts its restart policy logs `service gave up`.
- `--drain-timeout <s>` / `USAI_DRAIN_TIMEOUT` (default 30 s); the server's
  connection bound follows it. Sockets closed at the drain bound get `1012`.
- Logs: `--log-format json` writes one object per line on stderr; application
  lines carry `workload` and `target: "app"`; one-shot commands no longer echo
  lines twice; colour only on a terminal.
- `usai_resource_quarantines_total` is a counter of its own.
- Fuzzing: coverage-guided targets over the manifest, the HTTP boundary and
  source maps (`fuzz/`), two minutes per push and ten nightly in CI.

### SDK (`@sakaladev/usai`)

- **`ctx.resources` is typed from `resources: [...]`** (`ResourcesOf`); an
  undeclared name is a compile error.
- `summary` / `description` on every workload and `description` on auth
  schemes reach `inspect`, the reference page and OpenAPI;
  `defineApp({ description })`.
- `http.raw({ responses })` reaches the manifest (it was dropped);
  `RawContext.request` documents `bytes()`, `text()`, `json()`.
- `socket.accept` (sent by the SDK after auth) completes the upgrade; older
  bundles accept implicitly on the first receive/send.
- `usai/test`: `TestResponse.violations`; `app.queue(topic).deliver(message)`
  delivers one message to a consumer directly (the idempotency test).
- The SDK stamps the guest ABI it speaks (`builtWith.abi`); the runtime
  refuses a mismatch at install instead of faulting every request.

### CLI

- `usai probe live|ready [--addr]` exits 0/1 for container healthchecks
  without curl (the production compose uses it).
- `usai queue run <topic> --message '{...}'` delivers one message to a
  consumer in a fresh world.
- `usai dev` survives a failed first activation (503 `no_active_revision`,
  the error names `.env`) and watches the project root, so creating or
  editing `.env` activates a new revision.
- `usai inspect` groups workloads by kind; the graph edge reads
  `hands work to →`; `usai db seed` prints the seeder's return value;
  `usai generate openapi --public` writes the consumer contract.
- The documented environment forms of boolean flags (`USAI_NO_CRON=1`,
  `USAI_DIAGNOSTICS=1`) are accepted (only `true`/`false` were).
- `usai run --diagnostics` (`USAI_DIAGNOSTICS`) exposes error detail to clients
  for debugging.

### Documentation

- `/_usai/docs` is the **Application Reference**: one page per operation,
  stream, socket, task, cron, consumer, service, command, resource and schema;
  the lifecycle strip as the linked centrepiece; search; a request panel that
  sends real requests; cURL/JavaScript/Python snippets; light/dark; phone
  layout. OpenAPI gains the **public profile**
  (`/_usai/openapi.json?profile=public`).
- `docs/sdk/` — the SDK reference generated by TypeDoc (`make docs`).
- `SUPPORTED.md`: deployment topology (replicas × cron/queue/services), the
  **host envelope** (192 MiB / 1 vCPU per instance, 48 MiB runs hello), many
  applications per host.
- GUIDE: what runs where, browser WebSocket credentials and CORS at the proxy,
  what the app listener exposes, readiness vs services, no access log by
  design, the envelope, `errors.unavailable` for a dependency that is down.
- Runbooks updated (memory pressure, overload, restart, deploy/rollback);
  production compose example with the status token, surfaces, `usai probe`.

### Qualification (evidence, not features)

- P5/P6 on a production-shaped deployment: 13 deliberate failures, 1 000
  revision replacements under load, DB flap, restart loop, overload,
  dead-letter, **24 h soak** (35.0 M requests, 0 runtime errors), the
  **connection campaign** (SSE + WebSocket through replacement, restart, proxy
  restart, abrupt death, unread streams, idle sockets), the **two-replica
  campaign** (HTTP and queue sharing, concurrent migrations, cron on one,
  rolling restart, one replica killed).
- P8 parity report and the bench suite (six workload classes, Node/Bun/Deno/
  Rust/PHP comparators doing the same work); P8E efficiency envelope (floors,
  residency, density vs Node).
- Formatting: Biome for TypeScript/JavaScript/JSON in `make fmt` and CI.

### Compatibility

- Artifacts built by 0.0.5 run on the 0.0.6 runtime (`GUEST_ABI` 1 unchanged);
  a 0.0.6 artifact on a 0.0.5 runtime is refused at install with both versions
  named, as before.
- New environment variables: `USAI_STATUS_TOKEN`, `USAI_SURFACES_OFF`,
  `USAI_NO_CRON`, `USAI_NO_QUEUE`, `USAI_NO_SERVICES`, `USAI_DRAIN_TIMEOUT`,
  `USAI_DRAIN_GRACE`, `USAI_DIAGNOSTICS`, `USAI_SOCKET_IDLE_TIMEOUT`. One
  default changed: a SIGTERM now takes 2 s longer to close the listener
  (`USAI_DRAIN_GRACE=0` restores the old behaviour); orchestrator grace
  periods sized as drain + 5 s still cover it.

Draft tag message:

> 0.0.6: hot path halved (direct-call guest ABI, validate once), zero idle
> cost, status token and surface switches, replica flags (--no-cron/--no-queue/
> --no-services), drain grace for zero-502 rolling restarts, lost-consumer
> queue reclaim, WebSocket auth before the upgrade, typed ctx.resources, the
> Application Reference and generated SDK reference, P5/P6/P8/P8E qualified
> (24 h soak, connection and two-replica campaigns)

## 0.0.5 — 2026-09-18

P4 primitives (`httpClient`, `crypto`, `password`, transactions), source
maps, type-checked builds, signed artifacts (`usai keygen`, `build --sign`,
`run --require-signature`), migrations in the artifact, 503 for unavailable
dependencies, bounded revisions, observability round two, P5/P6 qualified.

## 0.0.4 — 2026-09-18

The npm wrapper no longer probes itself under `pnpm`; scaffold test; Docker Hub
`sakaladev/usai`.

## 0.0.3 — 2026-09-18

Distribution: Docker runtime and dev images, the compose path, the
npm-assisted binary, the artifact ↔ runtime compatibility contract, SIGTERM
drain.

## 0.0.2 — 2026-09-18

Dogfood fixes from the first outside-developer run.

## 0.0.1 — 2026-09-18

Developer preview.
