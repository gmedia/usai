# Changelog

User-visible changes per release, newest first. Runtime, SDK and CLI ship
together at the same version; an artifact built by one version runs on the
runtime of that version and the next (`SUPPORTED.md` → Versioning). Within
`0.0.x` contracts may change between versions; each entry says what.

GitHub releases carry the auto-generated commit list as well; this file is
the human summary.

## 0.0.9 — Unreleased

### CLI

- **`usai queue status`, `queue prune` and `queue prepare`.** The queue table
  was the application's to operate with no tools for it: depth was a SQL
  query an operator had to write, nothing pruned the rows a finished message
  leaves behind, and a version that adds an index built it under a
  write-blocking lock at its first use. `status` reports rows per topic and
  state with the oldest wait and the oldest claim, `prune` deletes finished
  rows on request (`--state done|dead|all`, `--older-than`, `--dry-run`, and
  never by itself), and `prepare` builds the indexes with `CREATE INDEX
  CONCURRENTLY` before an upgrade would.

### Runtime

- **`console.log` in a declaration no longer fails the build.** The bridge
  routes a log through the host's control call, and the definition phase
  counted every control call as an operation — so a `console.log` at module
  scope was refused as "declarations must not perform I/O" (ADR-0009). In
  `usai dev` the failed rebuild left the previous revision serving, which
  looks exactly like a log line that vanished. Only real operations count
  now, and a declaration's logs are printed at `usai build` and on every
  `dev` rebuild (`target: "app"`, `phase: "definition"`). A declaration that
  performs actual I/O is still refused.
- **`fields` in a JSON log line is an object, not a string.** `--log-format
  json` is written by the runtime's own formatter now, which parses the
  application's structured fields once — `.fields.invoiceId` in `jq`, no
  second parse stage in a Loki pipeline. Lines that carried no fields no
  longer print `fields=""`, and an empty `request_id` is left out.

### Operability

- **CPU is accounted per workload** (`usai_workload_cpu_seconds_total{workload}`,
  and `gauges.guestCpuNsByWorkload` in the status document). The runtime
  measures guest CPU and deliberately does not schedule it (**ADR-0021**):
  the accounting is how an operator finds the workload whose share does not
  match its importance, and `concurrency:` plus a deadline is how they cap
  it.
- **The overload refusal names the level that is full** — it always did
  (`runtime.worlds budget exhausted (2 in use)`), but nothing pointed at the
  body; `docs/runbooks/overload.md` shows it and maps each level to the knob
  that sets it.
- **`usai inspect` shows what three ADRs said it would.** A task's line says
  `delivery: local, non-durable` and a consumer's says `delivery: durable`
  (ADR-0010 promised this and it was only ever in the document — the
  assumption that `dispatch` survives a restart is the natural one to make).
  An **Admission** block lists all four budget levels with where each is set,
  rather than the one it used to print (ADR-0012).
- **A resource's identity is logged when it opens** (`resource opened
  kind=… name=… fingerprint=…`, and `resource reused` at debug). Identity is
  the config plus the *resolved* environment, so a typo in a URL silently
  opens a second pool; the fingerprint is how an operator sees that happen
  across a deployment (ADR-0011).

- **A hand-copied scaffold template says so.** `templates/hello` still holds
  the placeholders `create-usai` fills in, so copying it by hand failed four
  steps later on an npm install of a package called `__USAI_VERSION__`. The
  build names it at the first command instead.

### Documentation

- **`docs/CONTRACT-FREEZE.md`**: what 1.0 would freeze, surface by surface,
  what is still moving, and the three ways to take the decision. The
  `0.0.x` sentence in `SUPPORTED.md` is what stops anyone building on this,
  and removing it is a maintainer's call rather than a gate.
- **The npm compatibility budget** (`SUPPORTED.md`), which ADR-0013 asked for
  and nobody had written: what an application may bring from the registry,
  what it may not, and the promise that we will not polyfill Node globals to
  make a package work.
- Three runbooks the audit found missing: a compromised token or signing key
  (the one case a restart does not fix), the application's own failures (a
  service that gave up, `detached_work`, 504, cron that did not run, restart
  storms) and PostgreSQL operations (backup, restore — restoring `usai_queue`
  replays its work — connection limits, password and CA rotation).
- **ADR-0020** records the artifact-signing decision that shipped in 0.0.5
  without one, and says the manifest is the compatibility surface while the
  rest of the directory is a build output rather than an interchange format.

### CI

- The compatibility matrix gained its **third axis**: an application built
  against the *previous published SDK* is built and served by this runtime,
  or refused with the guest-ABI message before anything listens. The job also
  fails rather than skipping silently when a release exists and its binary
  cannot be fetched.
- **Secrets are scanned** on every push (TruffleHog, verified findings only).
- **Two fuzz targets for the two blind spots the fuzzing report named**: the host's side of the guest
  bridge protocol (`guest_bridge`) and the JSON → wire-type conversion every SQL parameter goes
  through (`postgres_params`). 3.8 M and 2.8 M executions on their first runs, 0 crashes; both are in
  the nightly matrix.

## 0.0.8 — 2026-09-23

Three usability rounds on 0.0.7 — a developer joining an existing codebase, an
Express developer bringing their folder layout and their query builder, and one
given **no documentation at all**, allowed to learn only from the CLI and its
error messages — plus the first report from an external developer. Everything
below was reproduced here before it was changed.

### Security-adjacent

- **A typo in a route's options no longer ships.** Where the options parameter
  is the inferred type itself (HTTP routes and streams, so `ctx` and the
  response type can be read off it), TypeScript's excess-property check never
  fired: `Auth: session` for `auth: session` compiled, built, and served the
  route **with no authentication at all**, and `respones:` silently dropped a
  response contract. Unknown keys are a compile error now. Every other
  declaration surface was already checked — verified one by one.
- **`z.url()`, and any format Zod emits without a pattern, is enforced.** The
  host did not assert JSON Schema formats (the specification's default) and
  the world's fast path saw a node whose `checks` list is empty, because for a
  format the format *is* the node. `POST {"url":"not-a-url"}` was accepted
  while the OpenAPI document told consumers the field was a URI. Both halves
  check it now: the host before a world, the library's own check inside it.
- **Raw transaction control on a pooled handle is refused.** Every statement of
  `query/one/execute` leases its own connection, so `begin` opened a
  transaction on a connection the following statements did not hold — and it
  went back to the pool `idle in transaction` while the "transactional" insert
  committed elsewhere, with no warning. `begin/commit/rollback/savepoint/
  release/set/reset/discard/listen/deallocate` now answer `transaction_control`
  and name `transaction(async (tx) => …)`, which accepts all of them.

### Runtime

- **A request that passed its deadline answers `504`, not `500`.** The world
  driver treated a guest error as a fault even when the deadline had already
  elapsed, so a timed-out request's status depended on where the watchdog's
  interrupt landed. Two branches now follow the rule the rest of the driver
  already had.
- **Timers expire from the moment the application asked.** The deadline was
  taken at the operation future's first poll, which on a starved runtime can
  be much later than the `setTimeout` call — two timers started in the same
  turn could then fire out of order.
- **Activation pays the first world's costs, not the first request.** A first
  request cost 16–21 ms against ~4 ms for the ones after it: the revision's
  routing table was compiled on the request path, and so was the engine's
  first instantiation of the image. Both happen at activation now, before
  readiness turns true (a first request is 7–11 ms, the rest is the guest's
  own first entry into your code).
- **`performance.now()` is a real monotonic clock.** The WASI clock shim
  ignored the clock id, so it answered with the wall clock: it reported the
  time since the *snapshot* was taken and stepped whenever the host's clock
  was corrected. It measures from the world's own start now.
- **A hand-off nobody declared is logged.** `ctx.tasks.dispatch(task, …)` from
  a workload with no `dispatches(...)` declaration still happens — the
  declaration is description, not permission — but it now writes one WARN per
  pair with the fix, because `usai graph`, `usai inspect` and the reference
  read the definition and cannot show an edge that is not there. `usai graph`
  says what it draws from.
- **A signal that arrives with the first request drains.** The SIGINT/SIGTERM
  handlers were installed when the shutdown `select!` was first polled —
  after the listener was already answering — so a signal in that window took
  the process's default action and killed it without draining. They are
  installed before anything is served now. An orchestrator that starts a
  container and changes its mind lands exactly in that window.
- **An application with no workloads says so** at build and at activation,
  instead of building, activating and answering 404 to everything.

### SDK

- **SQL parameters are `readonly SqlParam[]`**: nothing mutates them, and every
  query builder used as a compiler returns a `readonly unknown[]`, which needed
  a cast at each call site.
- **The Node reflexes are declared so the compiler can answer them.**
  `performance` was missing (correct code did not compile) and `process`,
  `require`, `Buffer` and `__dirname` were not declared at all, so TypeScript
  recommended installing `@types/node` — after which `process.env.X` compiles
  and is `undefined` at runtime. Their types are now named after the answer
  (`ThereIsNoProcessInAWorld_UseCtxEnv`).
- The published package ships **declaration maps**, so "go to definition"
  lands on the implementation rather than stopping at the `.d.ts`.

### CLI

- **A taken port is reported before the build**, not after it: `run` and `dev`
  bind the listener first (70 ms instead of 14 s).
- **A bundle failure explains itself.** Above esbuild's list: a package that
  reaches for a Node built-in when it is loaded cannot be part of an
  application (naming the packages), the runtime never scans your files, and
  top-level `await` is not available in an application module.
- **`usai dev`'s banner teaches the three rules** an Express instinct breaks
  first — a world per unit of work starting from a snapshot of module scope, a
  connection leased per statement, and `dispatch` for work that outlives the
  response. Two of the three are silent when violated.
- **The npm launcher explains a binary that will not start.** It verifies the
  binary it just fetched, does not leave one in the cache that cannot run, and
  turns the dynamic loader's `version 'GLIBC_2.x' not found` into the floor,
  the distributions that meet it and the two ways out (the Docker image, or
  `USAI_BIN` pointing at a binary built locally).

### Compatibility

Four of the fixes above change what a running application sees. None needs a
migration, but two can change a live response, so read them before rolling:

- **A request that was accepted may now be a `400`.** Declared string formats
  are enforced (`z.url()`, and anything else Zod emits as a `format` without a
  pattern). If clients have been sending values your schema said were URLs and
  were not, they were reaching your handler; they now fail at the boundary.
  Check the OpenAPI document for the formats you declared — that is exactly the
  list — and count `validation_failed` after the roll.
- **`begin`, `commit`, `rollback`, `savepoint`, `set`, `reset`, `discard`,
  `listen` and `deallocate` through `query/one/execute` now answer
  `transaction_control` instead of half-working.** Code that issued them was
  already broken (the statements landed on different pooled connections and the
  connection went back to the pool mid-transaction), but it was broken quietly
  and is now an error. `grep` for them and move each one into
  `transaction(async (tx) => …)`, which accepts them all. A `SET LOCAL` inside
  `transaction` is unaffected.
- **`performance.now()` returns a different number.** It was the wall clock
  since the snapshot was taken; it is now milliseconds since the world started.
  Code that treated it as a timestamp (added it to an epoch, stored it) must use
  `Date.now()`; code that measured a duration with it was already correct and
  gets a better answer.
- **A build can now fail on a file that compiled before.** An unknown key in a
  route's or stream's options is a type error rather than a silently ignored
  key — which is the point, since `Auth:` for `auth:` served an unauthenticated
  route. If CI fails here, the key it names was doing nothing.

Also new in the log, not an error: a WARN per (workload, task) pair when a
workload dispatches a task it never declared with `dispatches(...)`. Declare the
edge, or ignore it — the hand-off is unaffected.

### Documentation and examples

- GUIDE §3 states the rule nobody could find — **a workload exists because
  `defineApp` can reach it through an `import`; the runtime never scans your
  files** — and shows both an Express-style file layout and the
  routes/service/queries layering, with the one rule the runtime adds: a
  repository takes its `SqlExecutor` as an argument, so the same function works
  on the handle and inside a transaction.
- GUIDE §7 gains **"Query builders, and what you can npm-install"**: the rule
  that decides every package (a library that only computes is fine; one that
  imports a Node built-in at load time is not), kysely's `DummyDriver` and
  drizzle's `QueryBuilder` used as compilers — both served real rows through
  `ctx.resources.db` — and `knex`, which cannot be bundled in any form.
- Three statements in the GUIDE were **false** and are corrected: an auth
  resolver's `ctx.resources` is typed, so is an `httpClient` handle (§11 showed
  a cast it does not need), and `--conditions=usai` does not "do nothing" for
  an installed package — it breaks it.
- The examples no longer teach `as PostgresHandle` or `params as never`, and
  their `test` script runs `usai test`, which works outside this repository.

> 0.0.8: three usability rounds and a threat-model campaign. A typo in a
> route's options no longer serves an unauthenticated route (unknown option
> keys are a compile error), declared string formats are actually enforced,
> raw transaction control on a pooled handle is refused instead of stranding
> a connection, a signal arriving with the first request drains, activation
> pays the first world's costs, performance.now() is a real monotonic clock,
> and the bundler, the dev banner and the GUIDE teach the model instead of
> assuming it. Read Compatibility before rolling: two of the fixes can change
> a live response.

## 0.0.7 — 2026-09-23

A fix release: **0.0.6's Linux binaries and Docker images do not start on
Debian 12 or on any glibc older than 2.39.** Nothing else changed — same
runtime, same SDK, same contracts as 0.0.6.

### Fixed

- **The published Linux binaries are compiled on Debian 12 again.** They are
  built on the CI runner, whose glibc is newer than the floor this project
  promises (`SUPPORTED.md`: glibc ≥ 2.36), and Rust's standard library binds
  `pidfd_spawnp`/`pidfd_getpid` from whatever glibc it is built against — so
  every published tarball up to and including 0.0.6 asked for `GLIBC_2.39`
  and died with `version 'GLIBC_2.39' not found` on Debian 12, Ubuntu 22.04,
  RHEL 9 and Amazon Linux 2023. Until 0.0.6 the Docker images hid it (they
  compiled their own binary from source); 0.0.6 started shipping the
  published bits in the image, which made the images fail the same way —
  including the build stage of the scaffold's Dockerfile, where it surfaced
  as `usai: … GLIBC_2.39 not found` at `usai build`.
- **The release now refuses to publish a binary above the floor.** Each Linux
  binary is checked for its highest required glibc symbol version and started
  inside `debian:bookworm-slim` before it is uploaded.

### Compatibility

- **Upgrade if you run 0.0.6 anywhere but Ubuntu 24.04 (or newer).** The
  0.0.6 tarballs, the 0.0.6 npm install (the launcher fetches the tarball)
  and `sakaladev/usai:0.0.6` / `:0.0.6-dev` / `:latest` are all affected;
  `:0.0.5` and earlier images are not.
- No artifact, ABI or configuration change: an artifact built by 0.0.6 runs
  on 0.0.7 unchanged, and `GUEST_ABI` stays 1.
- `@sakaladev/create-usai` 0.0.10 scaffolds against SDK 0.0.7.
- `SUPPORTED.md` no longer claims Ubuntu 22.04: the floor is glibc 2.36
  (Debian 12, Ubuntu 24.04), which is what the binaries are now built for.

> 0.0.7: the published Linux binaries and Docker images are built on Debian 12
> again — 0.0.6 shipped binaries that required GLIBC_2.39 and would not start
> on Debian 12, Ubuntu 22.04, RHEL 9 or Amazon Linux 2023 (including inside
> our own images and the scaffold's build stage). The release now verifies
> the glibc floor before it publishes. No other change.

## 0.0.6 — 2026-09-23

**Withdrawn on Linux — use 0.0.7.** Its binaries and images require `GLIBC_2.39`; the 0.0.7 entry above says why and what it changes. Everything below shipped in 0.0.7 unchanged.

Since 0.0.5 (2026-09-18). Alpha, production qualification in progress: the
24 h soak, the failure campaigns, the connection campaign and the
two-replica campaign and the 72 h soak all passed (73.6 M requests, 0 × 5xx,
0 quarantined, flat memory).

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
- **PostgreSQL**: a `bigint` beyond ±2^53 comes back as a string (a JSON number would
  round it); GUIDE §7 lists what the driver covers and what it does not (COPY,
  LISTEN/NOTIFY, session state).
- **Uploads and headers**: `multipart.parse(bytes, contentType)` splits a form body into
  fields and files; `defineApp({ headers })` sets static response headers on every
  application response (a handler's own wins).
- **Structured log fields**: a trailing plain object in `ctx.log.*`/`console.*` is its
  own `fields` attribute (JSON) on the log line, not text in the message.
- **Drain asks background work to stop before cancelling it**: dispatched tasks, cron
  ticks and queue messages see `ctx.signal.aborted` and their `ctx.sleep` return at
  the start of a drain (services and connections already did); a long task can record
  where it got to. The drain bound still cancels what has not returned.
- `http.stream(path, { contentType })` declares a non-SSE stream (CSV, NDJSON): the
  response's `content-type` and the OpenAPI document follow it. A stream handler that
  fails after the head is logged and counted (`usai_http_streams_failed_total`).
- Integer SQL parameters accept a numeric string (a `bigint` that came back as a string
  binds again as it came); `usai_cron_ticks.claimed_by` names the instance (`host:pid`);
  a task cancelled by a drain is logged as `cancelled: <reason>` (was `faulted / no
  outcome`); `/_usai/*` on a listener without the surfaces says where they live.
- **Bytes and text are native in the core.** `TextDecoder`/`TextEncoder` and the SDK's
  `bytes` helpers call C codecs installed in the guest core (`__usai_native`); `atob`/
  `btoa` are QuickJS-ng's own (the bridge used to shadow them with a quadratic
  JavaScript version). A 3 MB body decode went from a CPU-slice fault to 0.2 s. New
  core `91f178df…` (`guest/PROVENANCE.md`); `GUEST_ABI` unchanged.
- **Queue throughput ≈4× on fsync-bound disks.** A claim is now an index probe
  (`usai_queue_claim`; the planner sorted every ready row per claim before), and the
  queue's own marks — claim, done, retry — commit without waiting for the WAL flush
  (`synchronous_commit = off` for those statements only; a lost mark is one redelivery,
  which at-least-once already allows; a message's publish and the handler's writes keep
  their synchronous commits). An empty claim retries once before sleeping.
- **Queue publish no longer prepares the schema on every call.** `CREATE TABLE IF NOT
  EXISTS` + `CREATE INDEX IF NOT EXISTS` ran per publish (two DDL statements and their
  locks); now once per database per process, retried once if the table vanished.
  Measured locally: 800 → 1 840 publishes/s from one world, 330 → 880 through a route
  at 16 clients (the queue campaign, `scripts/qualification/queue`).
- **Cron across replicas**: `cron(name, { exclusive: true })` runs each tick on exactly
  one instance — the schedulers claim the tick in PostgreSQL (`usai_cron_ticks`) and
  the others count it as `taken`; refused at install without a postgres resource.
  `/_usai/status` and `usai_cron_ticks_total{state}` expose cron counters.
- **Request ids.** `x-request-id` accepted from the client (sanitized) or minted;
  on the response, on every log line of the world (`request_id`), on outbound
  `httpClient` calls, and `ctx.requestId` in HTTP-shaped handlers.
- **The request body bound is a knob**: `USAI_MAX_BODY_BYTES` (1 MiB default); the 413 names it.
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
- From the runbook drill: a 5xx log line's error text is the `error` field
  (it was a second `message`, colliding with the line's own in JSON);
  `usai_http_request_seconds` is admitted requests only (refusals decided
  before a world are counted, not timed); `/_usai/status` →
  `resources[].ready` follows the last contact with the database (false with
  `lastError` on a connection-level failure, true after the next success —
  it stayed true through an outage) and is exported as
  `usai_resource{metric="ready"}`; a database outage writes one `WARN
  dependency unavailable … suppressed=<n>` per error code per second instead
  of an `ERROR` and a stack per request, and `INFO database reachable again`
  when it returns; the heap is returned after a password hash and after a
  revision removal (`malloc_trim` — an Argon2 burst used to retain 150–200
  MiB); a timed-out drain still retires the revision and says so (`revision
  retired cancelled_in_flight=<n>`); activating the active revision again is
  refused (it started a second scheduler); `invalid_state` messages name the
  states that would have been accepted; the control surface answers
  `404 unknown_workload` and `409 no_active_revision` instead of a 500.
- **The request id follows the hand-off.** A task a request invokes or
  dispatches runs in a world that carries the request's id: `ctx.requestId`
  in the task, `request_id` on its log lines and on its outbound calls. A
  queue message carries it too (`usai_queue.request_id`, a column added to
  existing tables on first use), so the consumer's world — even a retry
  minutes later — runs under the request that published it.
- Every missing environment variable is reported in one message
  (`missing required environment: DATABASE_URL, SESSION_SECRET`), not one
  per restart; invalid values likewise.
- OpenAPI: **declared errors are typed** — each declared status is the error
  envelope with `error.code` narrowed to the declared codes (an enum a
  generated client switches on) and its reason phrase (`Not Found: code
  not_found`; a `401` declared in `response:` used to read "Success");
  `x-usai-validated` says `both` for a slot that is refused at the boundary
  and parsed again in the world (a transforming schema); a route without a
  body no longer lists `invalid_json`; a raw `GET` has no `requestBody` and
  lists its path parameters; `204`/`205`/`304` carry no content; the socket
  description says what the browser does per auth scheme (a cookie travels
  with the upgrade by itself).
- `--log-format json` keeps stdout empty for `usai run`: the startup banner
  becomes one `serving` line on the JSON stream, so a log shipper that reads
  both of the process's streams no longer sees a parse error per start.
- Preparing the queue schema logs a warning when a statement takes more than
  half a second — on an upgrade that means an index building over an
  existing `usai_queue`, and the line says so.
- **A queue consumer's outcome mark is no longer lost silently.** The write
  that records `done`/`retry`/`dead` is retried once (synchronously) when it
  fails and then logged with what the operator will see — a message whose
  handler ran but whose mark never landed used to sit in `processing` until
  the sweep redelivered or dead-lettered it, with nothing in the log. Found
  by running a 0.0.5 and a 0.0.6 consumer against one `usai_queue` while a
  third process saturated the pool.
- Catch-all routes: `/files/*path` takes the rest of the path as one
  parameter; a catch-all serves the methods a literal path lacks
  (`http.options("/*any", …)` answers every preflight while `GET /users/:id`
  keeps its GET) and never turns an unknown URL into a 405.
- A client leaving an event stream is the normal end of the stream: logged
  at debug, not `ERROR`, and not counted in `usai_http_streams_failed_total`.
- A missing required property is reported at its pointer (`/name`), as the
  world's validator would, not at the object (`""`).
- A stream handler's own `cache-control`/`x-accel-buffering` values win
  (they were duplicated). Stream routes honour the boundary-final proof
  like HTTP routes.
- The reference page (`/_usai/docs`) explains CSV/raw/bodiless responses and
  the cookie/header/bearer cases for WebSockets.

### SDK (`@sakaladev/usai`)

- **`tokens.sign` / `tokens.verify`**: signed, self-describing bearer access
  tokens (`<payload>.<signature>`, HMAC-SHA256, base64url, `iat`/`exp`
  added, several secrets for rotation, constant-time verify) — verified in
  the resolver without a database hit. Not a JWT; `cookies.sign` stays the
  cookie form.
- **An auth scheme declares its own resources**: `auth.bearer({ resources: [db],
  resolve: async (ctx, token) => ctx.resources.db.one(…) })` — typed on the
  resolver's `ctx.resources`, and every workload that uses the scheme leases
  them in addition to its own, typed on the handler's `ctx.resources` as well
  (no more repeating the session table on each route, no more
  `resource_not_declared` from the one route that forgot).
- `TaskContext.requestId` and `QueueContext.requestId`: the id of the request
  that invoked, dispatched or published (empty for cron and `usai task run`).
- `usai/test`: **`app.logs(filter)`** and **`app.waitForLog(filter, timeoutMs)`**
  — the runtime's JSON log (the application's lines at INFO, the runtime's
  at WARN, `fields` parsed), filtered by `requestId`, `workload`, `level`,
  `target`, `message`; how a test observes a dispatched task. The harness
  now runs the runtime with `--log-format json` and `RUST_LOG=warn,app=info`
  by default and forwards the lines to stderr as before.
- Globals: `KeyUsage`, `CryptoKey`, `HmacImportParams` names, so code written
  against the DOM's WebCrypto types typechecks in a world.
- `operationId` on `http.*`, `http.raw`, `http.stream` and `socket` names the
  operation for generated clients; a socket's `incoming`/`outgoing` contracts
  are `components.schemas.<OperationId>Incoming/Outgoing` (public profile
  included). Error descriptions read `Not Found: code not_found` in both
  profiles.
- `responseHeaders: { 201: { location: "…" }, "*": { etag: "…" } }` on
  `http.*`, `http.raw` and `http.stream` documents the response headers a
  handler sets, per status, in the OpenAPI document (`responses[status].headers`)
  and the reference.
- `http.stream({ events: { tick: schema } })` declares a stream's events:
  `stream.event("tick", data)` validates the payload in the world
  (`event_contract_violation` ends the stream and names the event) and the
  document lists the events on the response with each payload typed as
  `components.schemas.<OperationId>Event<Name>`.
- `stream.event(name, data, { id, retry })` writes `id:`/`retry:` so a
  browser's `EventSource` resumes (`ctx.headers["last-event-id"]`); multi-line
  data becomes several `data:` lines.
- `http.notModified(headers)`, and a bodiless `HttpResponse<null>`
  (`noContent`, `notModified`) is accepted by a handler whatever its
  `response` contract — a `304` needs no `304: z.null()`.
- `cookies.sign` accepts values containing `.` (verify splits on the last dot).
- `usai/test`: **`app.stream(path)`** (server-sent events one by one — `event`,
  `data`, `json`, `id` — or `text()` for a CSV download; `close()` to leave)
  and **`app.socket(path, { headers, protocols })`** (a WebSocket the way a
  browser opens one: a cookie, or `["bearer", token]`; `send`, `next`,
  `closed`, `close`; `TestSocketRefused` with the status when the upgrade is
  refused).

- **`ctx.resources` is typed from `resources: [...]`** (`ResourcesOf`); an
  undeclared name is a compile error.
- `summary` / `description` on every workload and `description` on auth
  schemes reach `inspect`, the reference page and OpenAPI;
  `defineApp({ description })`.
- `http.raw({ responses })` reaches the manifest (it was dropped);
  `RawContext.request` documents `bytes()`, `text()`, `json()`.
- `socket.accept` (sent by the SDK after auth) completes the upgrade; older
  bundles accept implicitly on the first receive/send.
- **Cookies**: `auth.cookie({ cookie: "sid", resolve })` hands the cookie's value to the
  resolver (401 before the handler when missing); `cookies.parse/serialize/sign/verify`
  (Secure + HttpOnly + SameSite=Lax defaults, HMAC-SHA256 signing with rotation);
  response headers take an array value for a header that repeats (`"set-cookie": [a, b]`).
- **Bytes**: a `Uint8Array` SQL parameter binds to `bytea`; `bytes.toBase64/fromBase64`
  convert `bytea` columns and raw bodies; `usai/test` sends a `Uint8Array` body and every
  `TestResponse` carries `bytes`.
- `auth.custom({ credential: { in: "cookie" | "header" | "query", name } })`
  declares where a custom scheme's credential travels: OpenAPI emits an
  `apiKey` there and the reference's request panel sends it (a cookie via
  `credentials: include`). Without it the document no longer invents an
  `Authorization` header for a custom scheme — the operation is described as
  custom-authenticated and no security scheme is emitted.
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
- `usai inspect` prints each PostgreSQL resource's `pool.max` (with the
  default made explicit).

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
- Runbooks updated (memory pressure, overload, restart, deploy/rollback with
  the rolling-restart contract); new: **metrics reference** (every metric,
  labels, the alerts to set), **sizing** (worlds × pool × concurrency × memory ×
  replicas, with a worked example) and **systemd** (deploy, rolling restart,
  rollback, runtime upgrade on a plain VM); production compose example with
  the status token, surfaces, `usai probe`.
- GUIDE: cookies (a custom scheme with `credential`, `Set-Cookie` through the
  headers argument), the request body bound, uploads (raw bytes or presigned
  object storage; no multipart parser), binary columns (hex), the default
  30 s deadline and `504 deadline_exceeded`, and what the proxy owns — CORS,
  security headers, static files, the access log — with one Caddy block.
- GUIDE §4/§9: cookies in the browser (Secure on localhost and Safari,
  SameSite as the CSRF posture, `EventSource`/`WebSocket` and cookies), CORS
  in development (the dev server's proxy, or `http.options` +
  `defineApp({ headers })`), declare `errors` rather than error schemas,
  `z.strictObject` at the boundary, SSE ids; THREAT-MODEL names the browser
  posture (no CORS, no CSRF token, what stands in for one).
- GUIDE: access tokens and refresh-token rows, the lockout recipe and where
  per-IP limits live (§4); "decide inside the transaction, throw outside"
  (§7); the request id across hand-offs (§14); log access in tests and what
  `--conditions=usai` is for (§15). `AuthRequest`'s reference says where the
  resolver runs (inside the world) and what it must list.
- Two references: `docs/ENVIRONMENT.md` (every `USAI_*` variable — operating,
  tuning, tooling) and `docs/CONTROL-API.md` (the `--control` surface:
  install, activate, drain, remove, invoke, stop, with every error code).
- Runbooks corrected by the drill: postgres-down (idle connections vanish,
  only in-flight ones are quarantined; `sql_57p01` beside `connection_closed`;
  the rate-limited warning), runtime-restart (what the second and third
  signal do; 499 on the drain-timeout cancel; streams and sockets end at the
  drain start), metrics (`usai_service`, `usai_http_streams_failed_total`,
  lowercase state labels, the `<kind>:<name>` workload label), sizing and
  memory-pressure (per-workload memory: Argon2, bodies, held revisions), the
  systemd page (a real `%i` template unit with derived ports), and the
  production compose's Caddy block (`health_interval 1s`, at the drain grace).

### Upstream

- The pagemap slot-reset patch this runtime vendors — a complete pagemap
  traversal and the reset of dirty pages that are not resident — was
  **merged into Wasmtime `main`** on 2026-09-23
  ([#14357](https://github.com/bytecodealliance/wasmtime/pull/14357)). The
  vendored file is byte-identical to upstream; `vendor/` goes away with the
  first wasmtime release that carries it (v49.0.0 predates the merge).

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

**Upgrading a running 0.0.5 deployment, in order.** Each step is safe to
stop at; the details are below.

1. **Fix the alerts and dashboards first** (they must be right before the
   binary lands): the lowercase state labels, the queries that counted
   requests with `usai_http_request_seconds_count`, and the 5xx threshold if
   you run WebSockets.
2. **Prepare the database** (optional but strongly advised on a busy
   queue): prune `usai_queue` and create its new objects with
   `CONCURRENTLY`, days ahead if you like — a 0.0.5 runtime ignores all
   three.
3. **Install the binary and roll the replicas** one at a time, still serving
   the 0.0.5 artifact. Watch for an hour.
4. **Rebuild the application with the 0.0.6 SDK** and roll the artifact the
   same way.
5. **Rollback** is the reverse: the artifact first, then the binary — a
   0.0.6 artifact is refused by a 0.0.5 runtime, and nothing in the database
   has to be undone.

- Artifacts built by 0.0.5 run on the 0.0.6 runtime (`GUEST_ABI` 1 unchanged);
  a 0.0.6 artifact on a 0.0.5 runtime is refused at install with both versions
  named, as before.
- New environment variables: `USAI_STATUS_TOKEN`, `USAI_SURFACES_OFF`,
  `USAI_NO_CRON`, `USAI_NO_QUEUE`, `USAI_NO_SERVICES`, `USAI_DRAIN_TIMEOUT`,
  `USAI_DRAIN_GRACE`, `USAI_DIAGNOSTICS`, `USAI_SOCKET_IDLE_TIMEOUT`. One
  default changed: a SIGTERM now takes 2 s longer to close the listener
  (`USAI_DRAIN_GRACE=0` restores the old behaviour); orchestrator grace
  periods sized as drain + 5 s still cover it. `USAI_MAX_BODY_BYTES`,
  `USAI_CONTROL_TOKEN` and the tuning knobs are listed in `docs/ENVIRONMENT.md`.
- **Log pipelines**: a 5xx line's error text moved from a second `message`
  key to `error`; `fields` on an application line is a **JSON-encoded
  string, not a nested object**, so a Loki/promtail pipeline needs a second
  parse stage (`json` with `source: fields`); `--log-format json` now keeps
  stdout empty for `usai run` (the human banner became one `serving` line on
  the JSON stream), so a shipper that reads both streams no longer sees a
  parse error per start; a database outage is one `WARN dependency unavailable` per
  code per second (with `suppressed`) instead of an `ERROR` per request;
  `revision retired` now also appears after a timed-out drain (with
  `cancelled_in_flight`). Application log lines gain `request_id` and
  `fields`.
- **Metrics**, in the order they will bite:
  - State label values of `usai_revision_in_flight` and `usai_service` are
    lowercase (`active`, `failed`) — alerts written with `Active`/`Failed`
    match nothing and fire never.
  - `usai_http_request_seconds` no longer counts refusals decided before a
    world (a p99 may rise, honestly) — and therefore
    **`usai_http_request_seconds_count` is not the request rate**. A panel
    built on `rate(usai_http_request_seconds_count[5m])` now under-reports by
    the refusal volume, and *falls* during a flood of bad requests. Count
    volume with `usai_http_requests_total`, use the histogram for latency
    only.
  - **A WebSocket upgrade (101) is counted in `usai_http_upgrades_total`, not
    as a 5xx.** A deployment with sockets will see its 5xx rate fall by the
    upgrade rate: re-baseline 5xx thresholds before the upgrade, or they
    become meaninglessly loose.
  - New series (nothing breaks, but `sum by (…)` panels gain rows):
    `usai_process_*`, `usai_scheduler{kind}`,
    `usai_http_workload_responses_total{workload,class}`,
    `usai_resource{metric="ready"}`, `usai_cron_ticks_total{state}`,
    `usai_http_streams_failed_total`, and `reclaimed` in
    `usai_queue_messages_total{state}`. `docs/runbooks/metrics.md` is the
    complete list, with the label values as the exposition spells them (an
    HTTP workload label is `http:GET /invoices` — method and path, quoted).
- **WebSocket clients**: a refused credential is now a plain `401` to the
  upgrade request; 0.0.5 answered `101` and then closed the socket. A client
  that treats anything but `101` as a transport failure will retry forever
  instead of re-authenticating — check yours before the runtime upgrade.
  Existing credential forms keep working; `new WebSocket(url, ["bearer",
  token])` (the credential as the second subprotocol) is new and additive.
- **PostgreSQL.** On its first use of the queue (a consumer starting, or the
  first publish) a 0.0.6 process brings `usai_queue` up to date, in one
  statement each:

  ```sql
  ALTER TABLE usai_queue ADD COLUMN IF NOT EXISTS request_id text;
  CREATE INDEX IF NOT EXISTS usai_queue_claim ON usai_queue (topic, id) WHERE state = 'ready';
  CREATE INDEX IF NOT EXISTS usai_queue_processing ON usai_queue (topic, locked_at) WHERE state = 'processing';
  ```

  The `ALTER` is metadata only (a brief `ACCESS EXCLUSIVE` lock). **The two
  indexes are new in 0.0.6 and build under a `SHARE` lock: publishes and
  claims on that table block while they do.** The runtime never prunes
  `done`/`dead` rows (`GUIDE.md` §8), so on a long-lived deployment this can
  be minutes at the moment the first upgraded replica starts. Run the three
  statements yourself beforehand — with `CREATE INDEX CONCURRENTLY IF NOT
  EXISTS`, after deleting the rows you no longer need — and the runtime will
  find them and do nothing. A 0.0.5 runtime is unaffected by the column and
  never uses the indexes, so preparing days ahead is safe, and so is leaving
  them behind after a rollback. A slow statement now says so in the log
  (`preparing the queue schema took a while …`).
  `usai_cron_ticks` is created when a schedule is `exclusive`.
- **Both versions can share the queue table during the roll** — measured, not
  assumed: a 0.0.5 and a 0.0.6 consumer against one table, 100 messages
  published by the 0.0.5 CLI, every message processed exactly once
  (`docs/measurements/2026-09-18-p5-p6-qualification.md` → Mixed versions).
- **Rolling the binary back to 0.0.5 needs no database change**: the added
  column is nullable and 0.0.5 never names it, the added indexes are unused
  by it, and the migration ledger is untouched.
- **OpenAPI consumers**: error responses are `allOf [UsaiError, { error.code
  enum }]` rather than a bare `$ref` (a generator sees a narrower type, a
  reader sees the same envelope); descriptions read `Not Found: code
  not_found` in both profiles (the public profile used to say `Error code:
  not_found`); `x-usai-validated` has a third value, `both`; a raw `GET` lost
  its `requestBody`; new components `<OperationId>Incoming/Outgoing` and
  `<OperationId>Event<Name>` appear when sockets and streams declare them.
- **Test harness**: the runtime under `testApp` now logs JSON at
  `RUST_LOG=warn,app=info` (set `RUST_LOG` to change it); its stderr is
  forwarded as before. `HttpHandlerResult` accepts a bodiless
  `HttpResponse<null>` — code that narrowed on it may need a type
  annotation, nothing at runtime.
- **Auth resolvers** whose `ctx.resources` was cast keep working; declaring
  `resources` on the scheme removes the cast and the per-route listing.

Draft tag message:

> 0.0.6: hot path halved (direct-call guest ABI, validate once), zero idle
> cost, status token and surface switches, replica flags (--no-cron/--no-queue/
> --no-services), drain grace for zero-502 rolling restarts, lost-consumer
> queue reclaim, WebSocket auth before the upgrade, typed ctx.resources, the
> Application Reference and generated SDK reference, request ids across
> hand-offs (tasks and queue messages), cookies/tokens/bytes/multipart helpers,
> exclusive cron, native codecs in the core, auth schemes with their own
> resources, a test harness that reads the runtime's log, ENVIRONMENT and
> CONTROL-API references, P5/P6/P8/P8E qualified (24 h soak, connection,
> two-replica and queue-throughput campaigns, 78 M fuzz executions)

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
