# Changelog

User-visible changes per release, newest first. Runtime, SDK and CLI ship
together at the same version; an artifact built by one version runs on the
runtime of that version and the next (`SUPPORTED.md` → Versioning). Within
`0.0.x` contracts may change between versions; each entry says what.

GitHub releases carry the auto-generated commit list as well; this file is
the human summary.

## 0.0.10 — Unreleased

**If you are upgrading**, read this in two halves: what changes when the
**binary** lands, and what changes when the **rebuilt artifact** lands. They
are separate deployments, and the supported path runs the new binary against
the old artifact for a while (`SUPPORTED.md` → Versioning).

**When the binary lands, before you rebuild:**

- **One stream reaching its declared deadline used to break every other
  connection on the instance** — for one day, between two commits in this
  release. A stream was handed the revision's own drain token rather than a
  child of it, so its deadline cancelled the drain signal for the whole
  process: later sockets opened with `ctx.signal` already aborted and were
  closed `1012 server draining`, later streams were truncated to their first
  event, `/_usai/ready` kept answering 200 and the failure counter stopped
  counting. Fixed, and the test fails without the fix. It never reached a
  release; it is written down because the shape — a per-connection signal
  taken from a shared token — is one to watch for.

- **A `timeout:` declared on a stream is honoured — including in an artifact
  built by the previous SDK.** It used to be accepted and dropped. The world
  is stopped at the deadline, so the handler's loop sees `ctx.signal` abort
  and can finish; a handler that does not check simply stops writing. The
  client **keeps the `200` it already has**, the body ends, and there is no
  trailer and no error — a downstream consumer sees a complete, successful
  response over a truncated file. Measured: an export that took 16.1 s and
  wrote 41 rows on the old binary wrote 21 rows in 8.0 s on the new one, and
  `curl` exited 0. **If anything of yours parses a streamed export, make it
  fail loudly on a short file before you deploy this.** `usai inspect` lists
  every workload's effective deadline and where it came from; run it first.
- **A stream's latency becomes its world's lifetime in the global latency
  histogram** — `usai_http_request_seconds_{bucket,sum,count}`, the series
  the Latency alert in `docs/runbooks/metrics.md` is built on. Measured on
  identical traffic (20 short requests plus one 8-second export): `_sum` went
  from **0.019 to 8.029**, and the p99 moved from the `le="0.005"` bucket to
  `le="10"`. **Your latency alert will fire on the first export after the
  upgrade.** Exclude streaming routes or widen the alert before you deploy.
  (The per-workload series named elsewhere in these notes is *new* here, so
  nothing of yours reads it yet.)
- **`usai_http_streams_failed_total` now counts a declared stream timeout as
  well as a handler failure**, and no counter separates them; the WARN line
  says which. If you alert on it, it goes from zero to one per export.
- **The socket `close` fix is not in the binary.** A push loop whose client
  left still skips `close` — and still leaks whatever the connection held —
  until the rebuilt artifact lands. Worse: the `ERROR socket handler failed`
  line that used to appear on every such disconnect is a debug line in this
  binary, so between the two deployments the leak is **silent**. If your
  sockets hold per-connection state, deploy the binary and the artifact
  together.
- **Four `process.*` keys leave `/_usai/status` on a host with no cgroup
  memory limit**: `memoryChargedBytes`, `memoryLimitBytes`,
  `memoryCeilingHits`, `memoryOomKills`. They were `0` before and `0` was
  false, but anything reading them by name must tolerate their absence — and
  they will be present on your containers and absent on your bare metal.
- **The JSON log timestamp goes from millisecond to microsecond precision.**
  A shipper with a strict `%3f` format stops parsing.

**When the rebuilt artifact lands:**

- **A service's declared deadline is how it ends, not a failure.** It was
  recorded as `DeadlineExceeded`, logged `ERROR service ended with failure`
  and charged against the restart policy — so a bounded service was dead for
  good after its **first** cycle under the default policy, and under
  `restart: { mode: "always" }` it ran 3 s out of every 131 and was exhausted
  after ten. It now ends as `stopped`, and the restart policy sees a normal
  end. The give-up line no longer says "restart policy exhausted" when there
  was no policy to exhaust.
- **A service waiting out a restart backoff is `restarting`, not `failed`.**
  `failed` is what an operator alerts on — `docs/runbooks/application-failures.md`
  says so — and it was published for the whole doubling backoff between two
  ordinary attempts, so the documented alert fired on every transient restart
  and the documented health check took a healthy instance out of rotation for
  as long as 128 s.
- **`/_usai/status` → `revisions[].services[].restarts` is that service's own
  count.** It was the supervisor's single counter reported per service, so a
  service that had never failed showed its neighbour's number.
- **A SQL parameter that will not encode is described, not quoted.**
  `cannot encode a 22-character string`, not the string — the value is
  request data (a token in a query string, an id from a body) and
  `docs/THREAT-MODEL.md` promises the log carries none of it. The statement
  is still named, and the other parameters are still excluded.
- **A `timeout:` declared on a socket or a service is honoured too** — the
  SDK used to drop it before it reached the manifest, so the runtime never
  saw it. A socket ends at its deadline the way it ends when its client
  leaves: the connection is closed and **`close` runs**. A service ends and,
  with `restart: { mode: "always" }`, starts again. There is still no
  *default* deadline for any of the three kinds.
- **Declaring `maxBodyBytes` where there is no request body fails the
  build**, with exit 1 and the workload named. That is a CI failure, not a
  deploy failure. Nothing could have declared it before this release.

- **An `httpClient` declared without a `baseUrl` stops reaching private and
  loopback addresses.** If any of yours calls an internal service through a
  client that does not name its destination, it starts failing with
  `destination_refused` — declare `allowPrivateNetwork: true` on it, or give
  it a `baseUrl`, before the binary lands. Clients that name their
  destination are unaffected, which is most of them.

**Before either of those, when you rebuild against the new SDK**, four
things can stop a project that builds today. All four are refusals of
something that never worked, and each names what it found:

- **Your test suite.** `testApp` no longer runs cron, queue consumers or
  services (a test run has to be deterministic, and parallel test files were
  taking each other's queue messages). A test that asserts on the *real*
  retry or dead-letter path needs `testApp({ schedulers: { queue: true } })`
  and a database of its own. Everything driven explicitly — `app.cron().run()`,
  `app.queue().deliver()`, `app.task().invoke()` — is unaffected.
- **`usai test` typechecks** both the application's project and the test
  files' project before it runs anything. A suite that was green over code
  that does not compile now fails; `--no-typecheck` is the escape hatch.
- **The SDK refuses declarations the runtime always refused**: `maxBodyBytes`
  on a workload with no request body, `concurrency` meaning two things on a
  queue consumer, `ctx.tasks.invoke(task)` without the input the task
  declares. These were compile-time lies, not behaviour changes.
- **An application composing modules** is refused when two modules declare
  the same workload id or the same auth-scheme name with different
  configuration, or when two *different* modules share a name. All three used
  to pass — the third silently, the second corrupting the OpenAPI document.

**Rolling back is two axes, not one**: the artifact and the binary. An
application's identity is stable across a binary swap for the same artifact —
verified for this release — so a rolling deployment comparing identities sees
one application in both directions. Rolling back only the binary leaves the
new artifact on the old runtime, which is not a supported combination.

Everything else is additive or a fix to behaviour that was wrong.

### Multi-tenancy and outbound requests

- **An `httpClient` without a `baseUrl` no longer reaches this host's own
  network.** That client exists precisely so the destination can come from
  the application's data — a tenant-configured webhook, which the guide
  endorses — which makes the destination attacker-influenced by
  construction. Measured before the fix: a handler fetching
  `http://127.0.0.1:<port>/_usai/status` from inside a request got the
  status document back, RSS, per-route counters, revision identity and
  resource fingerprints included; on a cloud host the same call reaches
  `169.254.169.254`. Loopback, the private ranges, link-local, unique-local
  and their IPv4-mapped spellings are now `destination_refused`.
  `allowPrivateNetwork: true` opts out where the internal address really is
  chosen at runtime, and a client with `baseUrl`/`baseUrlEnv` is pinned to
  one origin already and is unaffected. **If your application calls an
  internal service through a client that does not name it, declare the
  opt-out before you deploy this.** The check resolves the name and refuses
  if any address is internal; a hostile URL still wants an egress proxy,
  and `docs/THREAT-MODEL.md` says so.
- **`USAI_PRECOMPILED=0` warns that it changes when your code runs.** It is
  documented as a loader and security switch, and it is also a semantic one:
  with the image, module scope is evaluated once at *build* and frozen into
  the artifact; without it, once per *process*. Two supported configurations,
  two behaviours for the same source, and nothing said so — the shape of bug
  that reproduces in staging and not in production.
- **GUIDE → Multi-tenancy**, the page a SaaS team needs: what a fresh world
  per unit of work already guarantees (measured: module scope, a
  module-level `Map`, a class static, a `WeakMap` and `globalThis` all start
  clean in every kind of world, under concurrency), row-level security with
  `set_config` inside `sql.transaction` on a restricted role, a second
  privileged resource for back-office work, the CI gates over `usai inspect
  --json`, and the sharp edges — `cache.local` is one flat key space shared
  by every tenant, admission is per workload rather than per caller, and
  nothing computed at module scope is per-instance.
- **The threat model no longer reads as "Usai does not isolate tenants".**
  "No isolation between applications or tenants" was about hostile *code*,
  and a reviewer deciding whether a SaaS may be built on this read it the
  other way. It now separates the two and points at the guide.
- **`docs/GUIDE.md`'s driver table said the pool leaks session state.** It
  claimed `SET` without `LOCAL` "would leak to the next lessee" — the
  opposite of what the runtime does, and pessimistic in the one direction
  that kills an adoption review. The session is reset on **every checkout**:
  measured with seven kinds of session state (a GUC, a temp table, a session
  advisory lock, `SET ROLE`, a prepared statement, `statement_timeout`,
  `search_path`) set on one backend and all clear on the next lease of the
  same pid — including after a deadline cancellation and after an aborted
  transaction.

### Modules and the shared layer

- **A module installed as a package owns its migrations.** The bundler
  skipped `node_modules` when it stamped each module's source directory, so
  a module *shipped as a package* — the way a shared layer is actually
  shared — had its `./migrations/*.sql` resolve from nowhere: `matches no
  file`, no `migrations/` in the artifact, and `usai db migrate` reporting
  success. A green deploy whose first request said `column does not exist`.
  A module inside `src/` or in a sibling workspace package was unaffected,
  which is what made it look like it worked.
- **`defineModule({ env })`.** A module can declare what it needs from the
  environment, and it is merged into the application's contract — so a
  missing value fails **activation**, like the application's own. There was
  no way to say it at all: every consumer had to mirror the variable into
  its own `env({})` by hand, nothing checked that it had, and `ctx.env.X`
  was `undefined` even with the variable set in the process environment. The
  application's own declaration of a key wins; two modules declaring one key
  differently is a build error.
- **Two auth schemes with one name is refused.** The name *is* the OpenAPI
  security scheme, so the first declaration seen described both: a cookie
  scheme documented as HTTP bearer, and every generated client for the other
  route sending `Authorization: Bearer` and getting 401 forever. The
  resolvers ran correctly, which is what made it invisible. Two modules each
  calling their scheme `session` is the default outcome, not an exotic one.
- **The same module listed twice is one module**, not a collision — the
  diamond every shared layer grows (`auth` and `billing` both re-export
  `base`) was a hard `duplicate workload id` with no de-duplication even for
  the identical object. Two *different* modules with one name, which used to
  pass in silence and made `inspect` attribute workloads to an ambiguous
  label, is now an error.
- **A duplicate workload id names who declared it**, on both sides
  (`declared by module "auth" and by module "billing"`). It was the id
  alone, which with two vendored module packages means a grep.
- **A declaration mistake reads like one.** `compile failed: validator
  warm-up failed: guest fault: Error: resource "main" is declared twice…`
  sent readers to the wrong place entirely; the application's declarations
  are evaluated in the guest, so a `defineApp`/`defineModule` error arrives
  as a guest throw. It is now printed as itself, and the resource conflict
  names its declarers the same way everywhere (`module "a"` and `the
  application`, not `a` and `app`).
- **`usai config` answers the question for a modular project.** It printed
  only the project's own globs — which in an application whose every
  migration comes from a module match nothing at all — and never mentioned
  the modules. It now lists each module, its source directory, its globs,
  and the effective migration glob list in order with where each came from.
- **`usai dev` watches a workspace module package's sources.** A module at
  `../../packages/auth` is outside the directory the watcher covered, so
  editing the shared layer changed nothing until you also touched a file in
  the application.
- **A task's input is typed at the call site.** `invoke(task, input?:
  unknown)` typed the output precisely and the input not at all, so calling
  a task that declares an `input` schema with no input — or with `42` —
  compiled, and the refusal arrived at the first run. For a module's task
  that is the half of its surface a consumer most wants held.

### Migrations and schema change

- **A migration can opt out of its transaction.** `-- usai: no-transaction`
  on a line of its own runs the file outside `BEGIN`/`COMMIT`, for the
  statements PostgreSQL refuses inside one: `CREATE INDEX CONCURRENTLY`
  above all, and `ALTER TYPE … ADD VALUE`, `VACUUM`, `REINDEX
  CONCURRENTLY`. Without it an application had no way to build an index
  concurrently at all — on a large table the ordinary form holds `ACCESS
  EXCLUSIVE` for the whole build, which is a write outage — while the
  runtime did exactly that for its own queue table (`usai queue prepare`).
  The ledger row cannot be atomic with the work there, so **the SQL must be
  idempotent** (`IF NOT EXISTS`): a process that dies between the file and
  the record runs the file again.
- **`usai db status` shows migration drift, and can gate a deploy.** It
  printed the checksum the file hashes to *now* beside the `applied_at` from
  the ledger, so a migration edited after it was applied was indistinguishable
  from an untouched one — in the table and in `--json` — while `usai db
  migrate` refuses that state outright. The runbook sends an operator to
  `db status` first ("everything applied and nothing pending is the only
  state to start from"), so it now names the drift, prints the ledger's
  checksum, and exits non-zero. `--check` additionally fails when anything
  is pending, which is the deploy gate everyone was writing in `jq`.
- **Concurrent `usai db migrate` no longer writes `duplicate key` ERRORs into
  PostgreSQL's log.** Exactly-once was settled by the ledger's primary key,
  so every migrator that lost a race left a constraint violation in the
  database's error log — and the documented deployment shape (an
  initContainer on every replica) makes that every rollout, indistinguishable
  from a real violation to anyone alerting on the error rate. The advisory
  lock is now taken *before* the ledger is read, so the loser finds the row
  and does nothing. Exactly-once is unchanged, and still per file, in its own
  transaction.
- **`usai app`, `usai task run`, `usai cron run` and `usai queue run` take
  `--artifact`.** A production image carries `.usai/build` and no source tree
  — that is the point of it — so `usai app import` inside one answered "not a
  Usai project: no package.json or usai.config.ts here", with a hint telling
  the operator to pass `--root`, which they already had. The only route to a
  declared command in production was the control surface of a *serving*
  replica. The `db` verbs have taken an artifact since D5; these now do too.
- **A one-shot that hits its deadline says which workload, how long it got
  and how to raise the bound.** It was `work ended without a result:
  DeadlineExceeded` — no name, no elapsed, and no hint that the 30 s it hit
  is the runtime's default for a workload that declares none. For a batched
  backfill run with `usai app` that is the difference between "declare a
  `timeout:`" and twenty minutes of believing the runtime faulted.

### Testing

- **Several `usai` processes on one project no longer destroy each other's
  build.** The staging directory is one path under `.usai/build`, so the first
  thing each build does — `remove_dir_all(.staging)` — deleted the files the
  other was writing, and a reader mid-rename loaded half a bundle (`the
  application bundle did not register __usai_sdk.describe`). That is not an
  exotic case: `usai test` runs test files **in parallel** and each
  `testApp({ migrate: true })` shells out to `usai db migrate`, so a project's
  *second* test file was enough — the CI recipe in GUIDE §15 did not work as
  written. Measured before the fix: four concurrent `usai inspect` on a cold
  project, one succeeded and three failed. Deciding "is the artifact current"
  and building now happen under one lock.
- **`testApp` does not run the background schedulers.** Cron, queue consumers
  and services are off by default, because a test run is supposed to be
  deterministic and they are not: a `* * * * *` schedule fires from the wall
  clock in the middle of a test, and — since test files run in parallel
  against one database — two files' consumers take each other's messages (one
  file saw four attempts of its own poison message, its neighbour saw two).
  `app.cron().run()`, `app.queue().deliver()` and `app.task().invoke()` drive
  that work explicitly. For the tests that need the real scheduler — retry,
  backoff and dead-lettering end to end — `testApp({ schedulers: true })`
  turns it back on; give those files a database of their own.
- **A killed test runner no longer leaves a runtime serving.** `close()`
  covers the happy path; a CI cancel or a `kill -9` does not call it, and the
  runtime went on holding its database pool until the machine was rebooted.
  The harness now starts it with `USAI_EXIT_WITH_PARENT=1`, and
  `--exit-with-parent` / `USAI_EXIT_WITH_PARENT` is available to any
  supervisor that cannot clean up after itself.
- **The runtime's `task failed` line carries the request id and the
  workload.** It had neither — only `task="task:send-welcome#d7"`, the
  dispatch's own id, which no `LogFilter` matches — so the documented way to
  observe fire-and-forget work (`app.logs({ requestId })`, "including the
  tasks it dispatched") covered only the case that went right. The failure is
  the one worth asserting on.
- **A `RUST_LOG` that names no level for `app` keeps the application's lines
  at INFO.** `RUST_LOG=warn` is the most ordinary thing a CI job sets, and it
  silently deleted every `ctx.log`/`console.*` line — which also blinded
  `app.logs()` and `app.waitForLog()`, whose failure then reads "0 lines
  seen" and points at the application. An explicit `app=` directive wins.
- **The harness prints the application's log, not the runtime's narration.**
  A 17-test suite had a wall of JSON between every two results. What reaches
  the terminal is the `app` target plus WARN and above; everything is still
  kept for `app.logs()`. `USAI_TEST_LOGS=all` or `=none` to change it.
- **`usai test` typechecks first** (`--no-typecheck` skips it), like `usai
  build` already did — **both** projects, the application's and the test
  files'. A suite could be green over code that does not compile, and the
  mistakes TypeScript catches here are the ones the runtime cannot: an option
  key that is not in the shape is dropped by the SDK, so `retry: { delay:
  "100ms" }` for `baseMs` ran the default and said nothing.
- **A failed `usai db migrate` in `testApp` shows what the command printed**,
  both streams, instead of `exited with code 1` alone.
- **`usai inspect` says where each contract is actually checked.** It printed
  "validated before world creation" for every declared contract, including a
  task's `input` and a socket's `message` — which are parsed **in the world**,
  because no host-side validator exists for those kinds — and then explained
  the in-world parse as "the schema transforms or refines" about schemas that
  do neither. `inspect` is the one source of truth about what the runtime
  understood (C8); it must not describe a boundary the runtime does not have.
- **The scaffold's `tsconfig.json` excludes `**/*.test.ts`**, so a test
  colocated under `src/` — which `usai test` discovers — is not a file the
  build refuses.
- **The SDK reference says which sources it describes.** `docs/sdk/` carried
  the package's version number and nothing else, so on `main` it documents a
  surface the published package does not have yet, and told readers the bug
  they were hitting was already fixed.

### CLI

- **`usai queue prune --dry-run` applies `--older-than`.** It counted by
  state alone and ignored the age, so the flag that exists precisely so an
  operator can read the number before typing `--yes` reported every finished
  row on the table — "this would delete 4.1 M rows" when the answer was 900.
  It runs the delete's own predicate now.
- **`usai inspect` prints a consumer's retry policy and concurrency.** The
  retry policy is the single most operational fact about a consumer and it
  was in `manifest.json` and nowhere a person would look, while cron in the
  same output printed `overlap:` and `exclusive:`.
- **`usai top` shows background work.** Its table was built from HTTP
  responses alone, so a queue consumer, a cron tick and a service were absent
  however hard they were working — measured, a consumer driving 441 worlds/s
  on a box at 126 % CPU read as a row of zeros and vanished entirely whenever
  no world happened to be live at the sample instant. Every workload that
  spent guest CPU in the window now has a row.
- **`usai inspect` prints the effective deadline** for every workload and
  where it came from (`timeout: 30000ms  (the runtime default)`,
  `(declared)`, or `none  (it runs until it ends or its connection does)`).
  The OpenAPI document has carried both since D2; the command that needs no
  server, and is the first one a new project runs, printed nothing unless
  the workload happened to declare one.
- **`usai top` has a `live` column** — worlds running right now, per
  workload. A workload with a world alive gets a row even when it has
  completed nothing at all, which is the case that matters: a runaway export
  moves no rate while it runs.
- **The JSON log line starts with `timestamp` again, at microsecond
  precision.** 0.0.9's formatter built the line in a sorted map, so the keys
  came out alphabetically — a line beginning `{"application"` breaks any
  shipper stage anchored on a prefix — and it rounded the timestamp to the
  millisecond, which made two lines in the same millisecond unorderable (a
  task and the request that invoked it, for instance). The order is
  deliberate now: `timestamp`, `level`, `message`, `target`, then the
  event's own fields. `fields` stays the object 0.0.9 made it.
- **`usai queue prune` counts unless you pass `--yes`.** Its defaults are
  `--state done --older-than 7d`, and a bare `usai queue prune` applied them
  immediately with no confirmation — a destructive default on a command
  whose own help says the runtime never prunes by itself. It now prints what
  it would remove and deletes nothing without `--yes`.
- **`usai top`** — what a running instance is doing *now*. `/_usai/status`
  is cumulative, which answers the wrong question during an incident: this
  takes two samples and prints the difference — requests per second, the
  average and the guest CPU per workload side by side (waiting or
  computing, answered in one screen), rejections that never reached a
  workload, pool `in use` and `waiting`, and what the process costs.
  A queue consumer's `req/s` is messages finished per second, from the
  per-topic counters: an upgrade round watched one finish 36 a second and
  read `0.0`, because the column was HTTP-derived.
  Memory is shown as the number that actually kills the process — the
  cgroup's charge against its limit, with RSS and PSS beside it, because RSS
  counts the pooled Wasm image once per slot it is mapped into and overstates
  by tens of MiB (measured on the 24 h soak: 89 MiB RSS, 66 PSS, 53 charged).
  A window in which the box hit its ceiling says so in its own line, since
  that is how a too-small limit fails *without* an OOM kill.
  Two lines sit above the table: the window's **p50 and p99 over all routes**
  (bucket bounds from the global histogram — the per-workload numbers are a
  sum and a count, and a mean hides a bimodal route), and **totals since
  boot**, because a rate screen cannot show an event that happened before its
  first sample and `-c 1` run right after an incident is exactly that case.
  The per-workload `cpu` column is a **share of one core** over the window
  rather than milliseconds per request: a stream or a socket completes
  nothing while it runs, so a per-request figure divided to zero and the
  workload eating the box read as free. `avg` is `—` when nothing completed,
  instead of a `0.00ms` that claims something about latency.
  The refusal lines name the reason on both clocks (`capacity 42.0/s` for the
  window, `(capacity 6)` since boot), because "6 refused" without one sends
  the reader to the wrong knob.
  `-n <seconds>` is the window, `-c 1` prints one screen and exits,
  `--addr`/`--status-token` read `USAI_STATUS_ADDR`/`USAI_STATUS_TOKEN` like
  `usai probe`. Counters that go backwards (a restarted instance) read zero,
  never a negative rate.
- **`usai probe --timeout <seconds>`** (3 by default, as before). An `exec`
  probe's own timeout has to be above the command's, or the supervisor kills
  the check instead of reading its answer — and until now the command's was
  both fixed and undocumented.

### SDK

- **`WorkOutcome.termination` is typed**, not `unknown` — it is the thing a
  lifecycle test asserts on, and asserting on it needed a cast. Its `logs`
  now carry the parsed `fields`, the workload and the request id, so a test
  holding an outcome no longer has to go back to `app.logs()` for them.
- **`LogFilter` admits `undefined`.** Its own doc comment recommends
  `res.headers["x-request-id"]`, which is `string | undefined`, and that did
  not compile under `exactOptionalPropertyTypes` — which this repository's
  `tsconfig.base.json` sets, so every example inherited the problem.
- **`WorkloadPolicies.timeout` says what it actually does** for each
  lifetime, now that a declared one is honoured on a stream, a socket and a
  service.
- **`ctx.tasks.invoke(task)` is typed from the task's handler.** It resolved
  `unknown`, so the first thing a service written from GUIDE §5 hit was
  `TS18046: 't' is of type 'unknown'` and a cast. `task()` now returns a
  workload that remembers what its handler returns (`TypedWorkload<Out>`,
  a phantom parameter — nothing changes at runtime, and it is still a
  `Workload` everywhere one is expected).

- **The declaration types say what each kind actually takes.** Every kind
  extended one shared policy set, so a task, a cron, a command, a stream, a
  socket or a consumer could declare `maxBodyBytes:` — which the runtime
  refuses at install, naming the workload, for everything that has no request
  body. The type now refuses it at the line that wrote it. A consumer's
  policy set is its deadline alone: `concurrency:` on a consumer is the
  topic's (how many messages it takes at once) and the shared set had been
  offering a second field with the same name and a different meaning.
- **`command()` forwards `concurrency:`.** It accepted the option and
  dropped it: the type said the budget existed, the manifest carried nothing,
  and `usai inspect` printed no budget where every other kind printed one.
  Two `usai app <name>` runs at once were bounded only by the application's
  budget.
- **`responseHeaders` no longer requires a `"*"` entry.** `Record<number |
  "*", …>` compiles to an index signature plus a *required* `"*"` key, so a
  route documenting only its `201: location` header did not compile until an
  unrelated catch-all was added.
- **`EnvValues<typeof spec>` — the cast the guide and `ctx.env`'s own doc
  comment both print — compiles.** `spec` is what `env({…})` returned and
  `EnvValues` took the field map one level inside it, so the documented line
  was a compile error for everyone who copied it. Both forms work now.
- **A raw endpoint's principal is on its context.** `http.raw` accepted
  `auth:`, ran the resolver and refused with 401, and `RawContext` had no
  `auth` — so the principal a signed-webhook endpoint had just proved was
  unreachable without a cast. Resources the auth scheme leases are on
  `ctx.resources` there too, as they are for every other kind.
- **`dispatches()` and `publishes()` keep the workload's own type.** They are
  annotations that return `from`, and they returned it as a bare `Workload`,
  so wrapping a task in either one — the idiom the guide prints — took
  `ctx.tasks.invoke` straight back to `unknown`.
- **`ctx.log.log` compiles.** `ctx.log` *is* `console` inside a world and the
  guest emits `console.log` at `info`; the type listed four levels and left
  out the one most people write first.
- **A cron's deadline is stated once.** The manifest carried `timeoutMs`
  twice for a cron — on the workload, where the runtime reads it, and inside
  the schedule trigger, where nothing did. The second copy is gone; a
  manifest built by an older SDK still installs, and its workload-level value
  is the one that was always honoured.

### Runtime

- **A long background job sees the drain, in both directions.** GUIDE §5 says
  "a long task that checks `ctx.signal.aborted` between steps records where
  it got to and returns, and the drain finishes when it does". It was wrong
  twice over. A **command or task invoked through the control surface** — the
  only way to run one inside a serving replica — was executed with no stop
  token at all, so `ctx.signal` never aborted: the job ran on through the
  whole drain (measured: 58 000 more rows backfilled *after* SIGTERM), was
  killed at the bound, and its return value was lost. A **cron tick or queue
  message already running** had the opposite problem: its cancel token was a
  child of the scheduler's stop, and that token means "stop claiming" and
  fires at the *start* of a drain — so the tick was cancelled outright 1.5 ms
  after SIGTERM, mid-batch. Now the one-shot paths get the same graceful stop
  a dispatched task has, and background work already under way is asked to
  stop and cancelled only by the drain bound, like every other in-flight
  world.
- **A socket or service stopped by its declared deadline gets five seconds to
  unwind, and a cut-off unwind is counted and named.** The grace was one
  second for every kind, which is not enough for an ending that has to do
  anything: a `close` acquiring a pooled connection can wait ten seconds for
  one before its statement starts, so the very handlers the stop-first design
  exists for — the ones with a row, a lock or a lease to release — were the
  ones cut off. Worse, the watchdog arms each guest entry with what is left
  of the deadline, and at the deadline that is zero, so the unwind's first
  synchronous run was interrupted after a **millisecond**. The grace now
  bounds the unwind in both senses, wall clock and CPU. A socket is detached
  at the stop and a service has no client, so its grace is entirely the
  handler's own ending; a **stream** keeps its response body open while it
  unwinds, so its grace stays at one second — there it is overrun on a bound
  the OpenAPI document publishes to consumers. A world that overruns the
  grace is cancelled — a bound is a bound — and now increments
  `usai_deadline_unwind_overruns_total` and logs a WARN naming the workload
  and the grace: a clean ending and a cut-off one both end as
  `DeadlineExceeded`, and only the second leaves something for the
  cancellation path to reclaim.
- **A `413` that abandoned the request body closes the connection.** The
  refusal happens mid-read, so the rest of the rejected body is still
  arriving on a socket the client believes it may reuse — and a keep-alive
  client does reuse it, so the reset lands on its *next* request: a valid one,
  on another route, failing for no reason visible from the client. The 413
  now carries `Connection: close`. (Found as an intermittent failure in this
  repository's own suite, which is the same shape any pooled HTTP client
  meets.)
- **`maxBodyBytes` on a route.** `USAI_MAX_BODY_BYTES` is one number for the
  whole process, so an import route that takes 5 MB opened every other route
  in the application to 5 MB as well — where an Express application would put
  `express.json({ limit })` on the one route. A route's bound is a **cap**,
  never a raise (the effective bound is the smaller of the two), so the
  operator keeps the ceiling and the application says how much of it each
  route may use; the 413 message names which of the two refused the request.
  Declaring it on a workload that has no request body — a task, a cron tick,
  a stream, a socket — **fails the build** with the reason and the workload
  named, rather than being accepted and ignored. (The refusal is in the
  definition, so an artifact that somehow carried one would be refused at
  install too; in practice you meet it in CI.)
- **Cron and queue consumers stop at the start of a drain, not after the
  grace.** The grace period exists so a load balancer notices the instance
  is unready while requests already in flight finish — nothing is watching
  a queue consumer, so claiming through the grace only took work this
  instance then had to finish while it was leaving. Measured by a jobs
  round: a new message claimed 1.67 s into a 2 s grace. What was already
  claimed still finishes.
- **A message dead-lettered by its contract writes a log line.** It was
  dead *silently*: a row in a table and a counter, with nothing on stderr and
  so nothing in a log pipeline. That is the shape of the commonest queue
  incident there is — a producer deployed with a new payload before the
  consumer's schema knew about it — and
  `docs/runbooks/queue-dead-letter.md` had promised the line all along. It is
  an ERROR now, like a handler that fails.
- **`usai_queue_topic_messages_total{topic,state}`**, and `queueByTopic` per
  revision in `/_usai/status`. Every declared topic's series exists from boot
  at zero, like the aggregate beside it — a dead-letter alert scoped to a
  topic had no series at all on a healthy idle instance, so the panel read
  "No data" rather than zero. An alert on dead letters could not name the
  queue, so whoever it woke had to look through every topic the application
  has — while HTTP has carried its per-workload dimension since D2.
- **A socket's upgrade is validated before the world exists.** `socket()`
  now takes `params` and `query` contracts, and the runtime checks them the
  way it checks an HTTP route's: a bad `/live/:board` is a `400` naming the
  slot and no connection is made. It was the one place on an HTTP surface
  where C6 ("fail before the world exists") did not reach — and it was the
  path segment that decides what the client is subscribing to. `ctx.params`
  and `ctx.query` are typed from the schemas when they are declared.
- **A WebSocket world learns that its own client left.** The only stop
  signal a socket had was the revision's drain, so a handler looping in
  `open` — which is how server push is written, and the only place to write
  it — never saw `ctx.signal` abort: measured by a realtime round, the loop
  kept running for 5.8 s past the client's close frame and kept writing to
  the database with nobody there. Each connection now has its own stop token,
  a child of the drain, and the connection ending cancels it.
- **`close` runs however the connection ended.** It ran only after the
  receive loop, which an `open` that threw never reached — and the normal end
  of a push loop is `ctx.send` rejecting with `client_gone` once the client is
  gone. So the one place an application can release what a connection held
  was skipped exactly when it mattered: 717 presence rows were left behind in
  one session. `close` now runs after a failed `open` too, and a client
  leaving is no longer reported as a handler failure.
- **A connection quarantined during shutdown is a debug line.** The drain
  cancels whatever a consumer had in flight and nothing can prove it
  terminal afterwards, so C5 quarantines it — correct, and the connection is
  being destroyed anyway. Logging it at WARN meant every green `usai test`
  run printed the one warning an operator must never learn to ignore.
- **A client going away is logged at debug, not ERROR.** A stream already
  said so; a socket logged `socket handler failed` per connection — 506 lines
  in the minute a round closed 250 dashboard tabs, which pages whoever alerts
  on the error rate.
- **Two commands in one project no longer race on the config bundle.**
  `usai.config.ts` was bundled to a fixed path, so a `usai dev` rebuilding
  while another command started could hand it a half-written module: the
  truncated config evaluated to a wrong application entry and the build
  failed with "the module has no default export" on a project that was fine.
  The bundle is written to a unique path and the evaluated cache through a
  rename. Found as an intermittent CI failure; it is a user-facing race, not
  a test one.
- **An artifact keeps its identity across a runtime upgrade**, and a test
  now holds the runtime to it. Identity is a hash of the manifest *as this
  runtime serializes it*, so a new optional field that is written even when
  absent silently changes the identity of every application built by the
  previous SDK — and a rolling deployment, which compares identities, would
  see two applications where there is one. `maxBodyBytes` below was one line
  away from doing exactly that. The test keeps a real 0.0.9 manifest and
  fails with the name of the attribute to add.
- **The queue schema survives being prepared by several replicas at once.**
  `CREATE TABLE IF NOT EXISTS` is not race-free in PostgreSQL, and the losers
  fail in three ways depending on the catalog they lost on: 42P07 (the
  relation), **42710** (the table's implicit row type) and 23505 (a unique
  violation on `pg_type`). The retry covered the first and the third. A CI
  run with several consumers starting together against a fresh database hit
  the second — `type "usai_queue" already exists` — which is exactly what the
  first publish from a new multi-replica deployment does. The test that
  exists for this raced nothing, because the table already existed when it
  ran; it drops the table first now, and fails without the fix.
- **A stream's latency is its world's lifetime.** It was recorded when the
  head committed, so a 6.5-second CSV export was filed as 2.8 ms — in the
  per-workload sum, in the global histogram and in `usai top`, which showed
  the slowest route on the box as idle. Every latency signal the runtime
  publishes was silently wrong for any application that streams. The number
  is now taken when the world ends; a stream's `Server-Timing` says
  `head;dur=…` instead of `total`, because the header leaves with the head
  and there is no honest total to put in it.
- **A `timeout:` declared on a stream, a socket or a service is honoured.**
  It was accepted, published to API consumers as
  `x-usai-timeout-source: "declared"`, and dropped — while `concurrency:` in
  the same options object was enforced. The runtime half of this shipped
  first and the claim was two-thirds false for a day: `socket()` dropped the
  option before it reached the manifest and `service()` had no such option at
  all, so only a stream was ever bounded and `usai inspect` told a socket
  that declared a deadline to declare one. Found by an upgrade round reading
  the release note against the tool. A connection-bound or persistent world
  is **stopped** at its deadline rather than cancelled, so a socket closes
  its connection and runs `close` — a declared ending is an ending, not a
  crash. An unbounded export whose contract
  promised it stopped after 8 s is worse than an unbounded export. There is
  still no *default* deadline for those kinds (one that ended a service
  after 30 s would be useless); a declared one ends the world, and for a
  stream that means the client's body stops there, counted and logged as the
  declared timeout rather than as a handler that failed.
- **A `504 deadline_exceeded` writes a log line.** It was a counter and
  nothing else: no workload, no request id, no duration — so "which request
  timed out at 03:14, and was it waiting or computing" had no answer unless
  the per-world debug trace happened to be on, which the runbook says not to
  leave on. It is now a WARN with `workload`, `request_id`, `duration_ms`
  and `cpu_us`, which is what `GUIDE` §4 and §14 had promised all along.
- **A failed build no longer bricks the artifact directory.** The bundler
  wrote `app.js` first, so a fault later in the build (a workload declared
  after the `defineModule` that lists it, for instance) left a new bundle
  beside the previous `manifest.json` — which the runtime correctly refuses
  to serve, permanently, until the next *successful* build. On a CI box or a
  deploy mount that directory is the rollback target. Everything fallible
  now happens in a staging directory and the artifact is published by
  renaming into it.
- **A bare calendar date binds as `timestamptz`.** `?from=2026-01-01` — the
  canonical reporting-API filter, ISO-8601, and something PostgreSQL casts
  without complaint — was `invalid_param: cannot encode "2026-01-01"`. It is
  midnight **UTC**, consistent with how the runtime already reads a
  zone-less timestamp.
- **A parameter error names the statement it came from** (`in: select …`,
  whitespace-collapsed, 120 characters, parameters excluded). An error
  raised by a host operation is not thrown from the handler's frames, so its
  stack is the runtime's; across thirty queries "which one" is the question,
  and now the message answers it. Arrays get their own advice instead of the
  scalar "pass a string in the type's text form", which for `_timestamptz`
  meant a PostgreSQL array literal and sent people in circles: bind
  `text[]` and cast the column in SQL.
- **`usai_workload_worlds_live{workload}`**, and `liveByWorkload` per
  revision in `/_usai/status`. A stream, a socket or a service holds a world
  for as long as it runs and completes no requests while it does, so the
  total alone could not say which workload was holding the budget.
- **`details` is no longer the JSON string `"null"`** on an `application
  error` line, and `stack` is absent rather than empty when there is none.
- **`console.time` / `console.timeLog` / `console.timeEnd` exist in a
  world.** They were missing, so instrumenting a handler by hand meant
  `Date.now()` arithmetic around every phase — and `console.time?.("x")`
  did not even typecheck. They log their elapsed milliseconds at INFO with
  the workload and the request id, like any other application line, so they
  are readable in production and not only in development. The labels are the
  world's: a timer started in one request cannot be ended in another, an
  unknown or duplicated label warns instead of reporting a meaningless
  number, and a timer left unended is forgotten when the world ends.
- **`--server-timing` (`USAI_SERVER_TIMING=1`).** `Server-Timing: total;dur=…,
  world;dur=…, cpu;dur=…` on every response the application produced — the
  request end to end, how long the world was alive, and how much of that the
  guest spent computing rather than waiting on a resource. Until now the only
  per-response timing was `--diagnostics`, which also hands clients error
  details and stacks and so cannot be left on; this is timing and nothing
  else. A request rejected before a world exists (C6) carries no header,
  which is the answer in itself. On by default in `usai dev`.
- **You can find a slow route now.** `usai_http_workload_request_seconds_sum`
  and `_count` per workload (and `latencySumSeconds`/`count` in
  `/_usai/status`): the global histogram cannot do it, because a route that
  is a minority of traffic never moves its p99 — measured, a 204 ms route
  left the documented latency alert reading 2.5 ms.
- **`usai_resource{metric="waiting"}`**, plus `usai_resource_operations_total`,
  `_transactions_total` and, for `httpClient`, `_requests_total`,
  `_failures_total`, `_refused_total`. `waiting` separates "the dependency is
  slow" from "my pool is too small", which are opposite fixes and were
  byte-identical in Prometheus before; the outbound failure rate could not be
  graphed at all.
- **A full PostgreSQL pool refuses instead of queueing without limit.** Every
  other admission level answers 503 at once, `usai inspect` prints the pool
  under that heading and the runbooks promised `resource_exhausted` — but the
  wait was unbounded, so 150 clients on a 2-connection pool got 410 × 200 at
  a **median of 23 seconds**, no refusal, no log line. The wait is 10 s now
  (`postgres("db", { pool: { acquireTimeoutSeconds } })`), and `usai inspect`
  says "queues, then refuses after 10 s".
- **`world trace` carries `request_id`**, so the runtime's own timing line
  joins the application's lines and the proxy's access log.
- **A cron tick is for a scheduled time, not for the moment the timer woke
  up.** A `*/1 * * * *` schedule fired **twice for the same minute** — once
  ~0.4 s early and once at the boundary, two real runs of the handler —
  because a sleep computed from the wall clock can end early and the
  scheduler took that as the tick, then recomputed the same occurrence. It
  waits out the difference now and refuses to fire a scheduled time twice.
  Present in every version before this one; `exclusive: true` schedules were
  protected by their claim row, every other schedule sent twice.
- **The status listener's 404 no longer names `/_usai/openapi.json` when
  `USAI_SURFACES_OFF=docs` removed it.** 0.0.9 fixed the banner and the same
  list one path further along was left behind.
- **An artifact built by a newer SDK than the runtime says so** (a WARN
  naming both versions, not a refusal — refusing would strand a rollback).
  The refusals are on the manifest format and the guest ABI, which do not
  move every release, so shipping the artifact before the binary used to
  work in silence.
- **`resource opened` is logged after the resource opens.** It was logged
  before, so a failed activation printed an INFO `resource opened` directly
  above its own fatal error.
- **The cgroup fields are absent from `/_usai/status` when there is no
  limit**, as the metrics endpoint already was: `memoryChargedBytes: 0` was
  simply false, and a dashboard could not tell "no ceiling hits" from "not
  measured".
- **The start-up memory check is a floor, not a forecast.** It used the full
  4 MiB per world slot and therefore fired at the 192 MiB envelope
  `SUPPORTED.md` tells operators to deploy on — training people to ignore a
  warning whose failure mode an orchestrator cannot see. It now warns below
  `40 + 30 + max_worlds` MiB (48 worlds → 118), which the measured cells
  agree with: 128 MiB runs the production shape with zero reclaim, 64 MiB
  runs hello with 94 events, 48 MiB thrashes at 1.4 million.
- **A refused signature says what to do about it.** "signed by X, which this
  runtime does not trust" now names both ways out (trust that key, or re-sign
  with the one the runtime was started with), and "does not verify" says the
  signature was made for different bytes than the artifact carries.

- **`USAI_ACTIVATION_RETRY=<seconds>`** (off by default, so nothing changes
  unless you ask). A bound resource that cannot be opened at activation ends
  the process — right for a missing variable, right on a VM, and on an
  orchestrator a restart loop whose backoff outlives the outage that caused
  it: a pod that restarts for an unrelated reason during a database blip
  cannot come back until the backoff expires, *after* the database is
  healthy. With a budget the process stays up and retries every 2 s, so a
  startup probe absorbs the blip and nothing restarts; when the budget ends
  it exits with the same error as before, and a missing or malformed
  variable is never retried. Raised by a platform engineer evaluating 0.0.9
  for Kubernetes, who called it the one change they would require before
  production; whether it should be the *default* is
  `docs/OPEN-QUESTIONS.md` → Q20.

### Documentation

- **`docs/deploy/grafana-dashboard.json`** — sixteen panels built from
  `runbooks/metrics.md`, including the two an operator has no other way to
  see: what the cgroup is charged against its limit, and the ceiling hits
  that mean the container is reclaiming the pages it executes from.
- **`docs/deploy/k8s/`** — the Deployment, the probes, the migration Job, the
  PodDisruptionBudget and the scrape target, plus the three decisions
  Kubernetes forces that a compose file never does: where the scheduler runs
  when every pod has the same PodSpec, when migrations run, and what a PDB
  means while the database is down. Until now the word "Kubernetes" appeared
  once in the whole documentation set.
- **A cron tick with no scheduler alive is lost, not replayed** (GUIDE §6,
  `SUPPORTED.md`) — true since the first version, written down only now, and
  the first thing to know before putting a single cron replica behind a
  `Recreate` rollout.
- **`usai probe` is for a Docker `HEALTHCHECK`**, not for a kubelet: an
  `httpGet` probe is made by the kubelet, so the image needs no HTTP client.
  The command's own timeout (3 s) is documented, and `USAI_STATUS_ADDR` must
  be the pod's address rather than loopback.
- **"Run migrations as a deploy step, never at startup"** was read as
  forbidding an initContainer. It does not: several migrators serialize on an
  advisory lock and apply each file exactly once. What it forbids is an
  application that migrates itself on its first request.
- Two sentences a reader would have coded against: a cancelled request's
  client gets `499` with a body rather than a closed connection, and
  `postgres-down.md`'s "Responses" bullet gave the *log's* wording for what
  the client sees (the client gets the code and `internal error`).
- `SUPPORTED.md` carries the container uid (10001) and the ~200 GB virtual
  reservation, where somebody writing a PodSpec looks for them.

### CI

- **The threat suite is 42 probes** (from 39). `--server-timing` is a new
  thing a response can say about the server, so three probes now assert what
  an ordinary response says without it: no `Server-Timing`, no `x-usai-*`
  (those belong to `--diagnostics` and `USAI_PROFILE`), and `x-request-id`
  returned, which is the one thing it should say. `docs/THREAT-MODEL.md`
  gained the matching entry — including that response timing is a side
  channel on a route whose body is careful not to be one.
- The secret scan failed on its own arguments (the action passes `--fail`,
  so passing it again is an error) — a job that fails on a flag looks exactly
  like a job that found a secret.
- A readiness test pointed its relay at the server's default database instead
  of the throwaway one its fixture had created, so the fixture's migrations
  ran into the database CI shares with the example suites and both example
  test runs failed on `relation "users" already exists`.

## 0.0.9 — 2026-09-23

> **Known issues in 0.0.9, found by an operator upgrading a real service
> from 0.0.8 and fixed in 0.0.10** (none is a data hazard; all are
> observability or noise):
>
> - The start-up memory check warns at the **192 MiB / `--max-worlds` 48
>   envelope `SUPPORTED.md` recommends**. It is a false positive — the shape
>   measures a 106 MiB peak with no reclaim — but if you page on `WARN` it
>   will page on every start. Re-scope that alert or raise the limit above
>   262 MiB until 0.0.10.
> - **The JSON log line's keys are alphabetical**, so it no longer begins
>   with `timestamp`; any shipper stage anchored on a line prefix needs
>   updating. Timestamps also lost microsecond precision, so two lines in the
>   same millisecond are no longer orderable.
> - **`usai queue prune` with no arguments deletes** `done` rows older than
>   seven days immediately. Pass `--dry-run` first; 0.0.10 makes counting the
>   default.
> - `/_usai/status` reports the cgroup fields as `0` when there is no memory
>   limit (the metrics endpoint correctly omits them), `resource opened` is
>   logged just before a failed activation says it could not start, and the
>   status listener's 404 still names `/_usai/openapi.json` after
>   `USAI_SURFACES_OFF=docs` removed it.
> - Not new in 0.0.9, but found the same day: a schedule could fire **twice
>   for the same period** (measured on `*/1 * * * *`) unless it was declared
>   `exclusive: true`. Fixed in 0.0.10.


A platform engineer deployed 0.0.8 from the published artifacts for the first
time — release binary, systemd, a proxy, two replicas, a signed artifact, a
token-gated status listener — and would run it in production. Both of the
serious things they found were **deployment guidance rather than the
runtime**, and both are fixed here. Alongside them, three campaigns of our own
turned out to be measuring nothing, which is the other half of this release.

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

- **A listener that does not serve a surface answers 404, not 401.** With a
  status token set and the surfaces on their own listener, the public
  listener answered `401 unauthorized` for `/_usai/status` and
  `/_usai/metrics` while answering 404 for `/_usai/live` and `/_usai/ready` —
  telling an unauthenticated caller that this is a Usai runtime with an
  operator surface to come back for, and naming the environment variable
  that guards it. Whether the listener serves the surface is now decided
  before the token is looked at.
- **A readiness probe that hangs now makes the resource unready.** The
  one-second bound lived in `/_usai/ready`, so a probe that never returned —
  the usual shape of an unreachable database — was *cancelled* before it
  could record anything: `usai_resource{metric="ready"}` stayed at 1 and the
  documented database-down alert waited for the next real query, which on a
  quiet service is minutes. The bound is inside the resource now.
- **A database that accepts the connection and then says nothing no longer
  hangs activation forever.** `connect_timeout` covers establishing the TCP
  connection and nothing after it, so a half-open firewall or a proxy in
  front of a dead backend left `usai run` waiting at startup with no
  diagnostic at all. The handshake is bounded now (twice the connect
  timeout), and the refusal says which of the two happened.
- **`USAI_READY_REQUIRES_RESOURCES`** (and the start-up log says when it is
  off). Readiness fails when a bound resource
  fails its probe, and a proxy removes an unready upstream — so a shared
  PostgreSQL outage takes out every route on every replica, including the
  ones that never touch it. That remains the default (a rollout should stop
  rather than go live broken); `0` keeps resource-free routes serving and
  reports the failing resource in the body instead. Draining fails readiness
  either way.
- **`usai run` warns when the memory limit is below what `--max-worlds` can
  need.** Only the process knows both numbers, so only the process can check
  them against each other; the line names the limit, the worlds and the
  arithmetic from `docs/runbooks/sizing.md`.
- **The runtime can see its own memory ceiling.** `/_usai/status` and
  `/_usai/metrics` read the process's cgroup when asked and publish the
  limit, the charge, the **ceiling hits** (`memory.events` `max`) and the OOM
  kills — only when there is a limit. The ceiling hits are the one number
  that identifies the state between "fits" and "OOM-killed": a container
  pinned at its limit reclaims the pages of the binary it is executing and
  faults them straight back in, with nothing killed and nothing logged. The
  campaign measured one request per second in a box 16 MiB too small. The
  alert is in `docs/runbooks/metrics.md`.
- **The startup banner announces only the surfaces that are served.** It
  listed `/_usai/docs` after `USAI_SURFACES_OFF=docs` had removed it, and the
  status listener's 404 quoted the same fixed list.
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

### SDK

- **A module's migrations and seeders may be written relative to the module.**
  `defineModule({ migrations: "./migrations/*.sql" })` in
  `src/billing/module.ts` now finds `src/billing/migrations/*.sql`: the build
  stamps each declaration with the directory it was written in, so a module
  is a folder you can move or copy. A glob is still tried **as written from
  the project root first**, so every application built before this is
  unaffected, and the "matches no file" warning fires only when neither form
  matches.

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
- **The multi-replica proxy configuration is published**
  (`docs/runbooks/deploy-and-rollback.md`), in full and copyable. It used to
  live in a qualification script, and the obvious translation of the
  runbook's own per-replica status ports into a Caddy block health-checks
  *every* replica on replica 1's port — one replica going down takes the
  others out of rotation. One loopback address per replica, identical ports.
- **Health-checking `/_usai/ready` couples every route to every bound
  resource**, and a shared database outage is therefore a total outage rather
  than the partial one the runbook promised. Both runbooks say so now, with
  the knob that changes it.
- Runbooks quoted **log lines that do not exist** (`WARN lifecycle
  violation`): a violation's message *is* its explanation, and the fact to
  match on is the structured `code` field.
- **Do not set `LimitAS` or `ulimit -v`** (`systemd.md`): the world pool
  reserves address space per slot — ~200 GB of virtual mapping for 48
  worlds, tens of MB of it resident — so an address-space cap kills the
  process at the first world. `MemoryMax` is the limit that means something.
- The scaffold ships **`migrations/0001_notes.sql`** and a commented
  `postgres()` block, so there is a production migration path to run before
  the operator has written any application code; `SUPPORTED.md` explains why
  `create-usai` has a version line of its own.

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

### Compatibility

No artifact, ABI or configuration change: an artifact built by 0.0.8 runs on
0.0.9 unchanged and `GUEST_ABI` stays 1. Three things a running deployment
should read before rolling:

- **`/_usai/status` and `/_usai/metrics` answer 404 instead of 401** on a
  listener that does not serve them (the application listener, when the
  surfaces are on `--status-addr`). Anything scripted against the 401 —
  a monitor that treated it as "the surface is there, I just need the token"
  — sees a 404 now. On the listener that *does* serve them the 401 is
  unchanged.
- **Check your multi-replica proxy configuration.** If a Caddy
  `reverse_proxy` with several upstreams carries `health_port`, it is
  health-checking *every* replica on one replica's status port, and losing
  that replica takes the others out of rotation with it. This is not a
  change in 0.0.9 — it is a latent misconfiguration the runbook used to
  invite. `docs/runbooks/deploy-and-rollback.md` now has the block that
  works: one loopback address per replica, identical ports.
- **The database-down alert fires at all.** `usai_resource{metric="ready"}`
  now flips within a second of the first failed readiness probe. Measured
  against 0.0.8 on the commonest outage shape — a database that accepts the
  connection and then stops answering — the 0.0.8 metric **never moved**
  while `/_usai/ready` was already 503, so an alert on
  `usai_resource{metric="ready"} == 0` did not fire during the outage at
  all. Do not retune that alert: check whether it has ever fired.
- **Three more changes an observability pipeline feels**, none of them in
  the first draft of this section: `/_usai/openapi.json` flips 401 → 404 on
  a listener that does not serve it, exactly like `status` and `metrics`;
  `usai_workload_cpu_seconds_total{workload}` adds one series per workload
  that has run, including `cron:` and `queue:` ones, so scrape cardinality
  grows with the application; and while `fields` becoming an object removes
  a parse stage for `jq`, a Loki `| json` stage *renames* every field it
  contained (`invoiceId` → `fields_invoiceId`), so saved queries, alert
  expressions and dashboard variables built on those names have to be
  rewritten before the roll, not after.

### Qualification (evidence, not features)

- **What a box does at its ceiling, when it is charged for it.** A 48 MiB
  `hello` cell was never OOM-killed, answered every request — and served one
  per second, with 1.4 million `memory.events.max`: it
  spent the cell reclaiming and re-faulting its own text. Its resident set
  was *smaller* than the cells with room to breathe. Under the old
  accounting that cell passed with "≈2 MiB of headroom".
- **The floors, re-measured.** Supported floor **192 MiB / 1 vCPU** stands
  (the production shape peaks at 106 MiB resident / 93 MiB charged, the same
  at 128, 192 and 256 MiB boxes, and at 128 MiB the kernel never reclaims
  once while it serves 1 098 req/s at p50 14 ms); **the technical floor is
  64 MiB, not the 48 MiB published before**. Every floor this project had
  published was void until this run.
- **Why they were void.**
  The boxed cells bind-mounted the binary they were measuring, and page cache
  is charged to the cgroup that first faults it in — the host had already run
  the same binary, so the box was never billed for the runtime's own text.
  The comparator run that exposed it reported 88 MiB resident inside a 48 MiB
  box with no OOM kill. Baking the binary into an image does not fix it
  (`docker build` warms the cache just as well, and 33 MB stayed uncharged
  either way): the harness now **drops the page cache for what the cell is
  about to run** — `posix_fadvise`, no privileges needed — so the box faults
  its own pages, and records what the kernel charged it: peak, ceiling hits,
  OOM kills, and whether the cache was really cold.
  A floor is read from the resident set and the reclaim events, never from
  the absence of an OOM kill — and it depends on the artifact: the cells use
  the `dist` binary, because once a box pays for its own page cache the
  file's layout is part of the result.
  `docs/measurements/2026-09-23-floor-accounting.md`. The comparator floors
  are still owed: they need an idle host, and run after the soak.
- **The threat suite is 39 probes.** Two were added the evening round 16
  found the 401: the production shape (surfaces on their own listener, a
  token set) must answer 404 for every `/_usai/` path on the application
  listener, and the body must name no operator surface.
- **A run of refusals is not a passing run.** The three-replica campaign
  reported PASS for three scenarios in which every single request was a 401:
  the tenant token was extracted with a `sed` that prints the whole body when
  it does not match, and the pass rules — no 5xx, no errors, both replicas
  created worlds — are all satisfied by refusals. Every load-driven verdict
  now requires that something was served.

> 0.0.9: an operator deployed 0.0.8 from the published artifacts for the
> first time and would run it in production; both of the serious things they
> found were deployment guidance rather than the runtime, and both are fixed
> here — the multi-replica proxy configuration is published in full, and what
> health-checking /_usai/ready couples is stated everywhere a health check is
> configured (with USAI_READY_REQUIRES_RESOURCES to decouple it). A listener
> that does not serve a surface answers 404 rather than 401, a readiness
> probe that hangs now marks the resource unready, a database that accepts
> the connection and then says nothing no longer hangs activation, and the
> queue table has operator verbs. Three of our own campaigns turned out to be
> measuring nothing: every floor this project had published was void — the
> boxes were never charged for the binary they ran, and re-measured, the
> technical floor is 64 MiB rather than 48 — and the three-replica campaign
> passed 11 of 11 only after the run before it passed on refusals. Read
> Compatibility before rolling.

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
