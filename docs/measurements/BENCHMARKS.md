# Benchmarks — method

> No single benchmark is the benchmark.

A runtime whose point is lifetime, ownership and failure behaviour cannot be
judged by one req/s column. The suite (`scripts/qualification/bench/suite.sh`)
asks three different questions with three kinds of workload and reports
four scoreboards; a reader who wants one number is reading the wrong document.

## The three questions

| Workload | Question | Where |
|---|---|---|
| **hello** (class A) | the runtime tax microscope: what a request costs when the handler does nothing | `suite.sh invoice` / `sweep` |
| **contracts and PostgreSQL** (classes B–F) | product economics: what the tax is relative to real work | `suite.sh invoice` / `sweep` |
| **soak, failure, churn** | production trust: what happens over days and under harm | `scripts/qualification/p5`, `p6` (`docs/measurements/2026-09-18-p5-p6-qualification.md`) |

Hello stays in the suite forever — when the hot path improves, hello is where
it shows first — but it is never quoted alone.

## Workload classes

Every class is one route of `scripts/qualification/bench/app` (a Usai
application written the way the GUIDE says to write one) and the same route
in every comparator (`baselines/`). The work is the same everywhere:
**validate the input, do the work, validate and encode the output**.

| Class | Route | Work |
|---|---|---|
| A | `GET /hello/:name` | params contract, constant body |
| B | `POST /orders/quote` | 12-field body contract, a small computation, response contract |
| C | `GET /users/:id` | params contract, one pooled `SELECT`, response contract (the research fixture's shape: EXP-011E/012B) |
| D | `POST /users` | body contract, `INSERT … RETURNING`, 409 on conflict |
| E | `POST /orders/:id/pay` | one transaction: `SELECT … FOR UPDATE`, `UPDATE`, `INSERT`, commit; 409 when already paid |
| F | `GET /me` | API key looked up in PostgreSQL, then class C |
| probes | `GET /counter`, `GET /slow` | cross-request state, cancellation |

`conformance.mjs` runs the 200/201, 400, 401, 404 and 409 cases of every
class against a server and the suite refuses to measure one that deviates
(status or parsed JSON; key order ignored). Before every D and E cell the
suite resets what writes leave behind (payments, `paid` flags, inserted
users), so every server pays the same orders from the same state and a 409
count is a property of the random ids, not of the run order.

## Comparators

| Server | Stack | Role |
|---|---|---|
| `usai` | the Usai runtime, the app's artifact | the subject |
| `node` | Node 24, Fastify, `pg` pool, Zod on input and output | the denominator (the research comparison used Fastify) |
| `bun` | Bun, Hono, `Bun.sql`, Zod | comparable framework, not a bare `Bun.serve` |
| `deno` | Deno, Hono, postgres.js, Zod | comparable framework, not a bare `Deno.serve` |
| `rust` | axum, deadpool-postgres, serde + explicit bounds | the attribution control: HTTP + PostgreSQL with no execution world; what Usai pays on top of it is the runtime |
| `php` | nginx + PHP-FPM 8.4 + PDO (persistent), docker | representative stack, reporting only, c=1 |

The JavaScript comparators validate input **and** output with the same Zod
schemas the Usai app declares (`baselines/shared-schemas.mjs`); the Rust
control enforces the same bounds by hand. Nobody is compared against
`return pool.query(...)`.

What is deliberately not equalised: Usai runs every request in a fresh
execution world with an ownership ledger; the others keep one process. That
difference is the subject. The `counter` probe makes it visible: Usai
answers `1` every time, the others answer the request number. Neither is a
bug.

## Modes

- `suite.sh invoice` — c=1, `DUR` seconds per class, the server pinned to one
  core (`PIN_SERVER`) and the client to another (`PIN_CLIENT`), as the
  research did. For Usai the server runs with `USAI_PROFILE=1` and the
  client averages the `x-usai-profile` header: the **per-phase invoice**
  (host HTTP phases, runtime/driver/engine phases, the guest's own ledger:
  dispatch, validation per slot, handler, response contract). This is the
  P8 gate: before and after every hot-path change.
- `suite.sh sweep` — c ∈ `CONCS` (1…64), every class, every comparator;
  oha for the read classes at c > 4, the suite's own closed-loop client
  (`load.mjs`) for writes and small c. PHP at c=1 only.
- `suite.sh leak` — the correctness probes: counter over ten requests,
  twenty aborted slow requests then health (and Usai's live worlds), RSS
  before/after ten seconds of class C.

Per cell: req/s, p50/p95/p99 (ms), **CPU per request** (utime+stime of the
server's process tree over the cell, divided by requests), RSS, outcome
counts. `report.mjs` renders the tables.

## The four scoreboards

1. **Performance** — req/s, p50/p95/p99, CPU per request, per class and
   concurrency.
2. **Efficiency** — RSS idle and under load, memory per request; density
   (many applications per host) later.
3. **Correctness / lifecycle** — cross-request state, resource reuse safety,
   cancellation, recovery: the probes here and the runtime's acceptance
   tests.
4. **Operations** — overload behaviour, restart/deploy loss, DB outage
   recovery, soak stability: the P5/P6 campaigns.

A result is reported as it came out. A number without its conditions
(host, pinning, co-tenants, versions, `DUR`, `CONCS`) is not a result.

## Reading the invoice

The invoice attributes one request's wall time (`x-usai-profile`, only when
`USAI_PROFILE=1`):

```text
http.route/decode/validate/admit/encode   the host, before and after the world
http.execute                              = runtime.create + driver.run + driver.retire + release (instance drop, slot reset)
engine.instantiate.*                      store, instance, exports, seed
engine.invoke.entry                       entering the guest (`__usai.entry`): the SDK's synchronous dispatch runs inside it
engine.invoke.settle                      the job queue drained in the core and the state read back (`qjs_usai_settle`)
engine.deliver.complete / deliver.settle  a host-operation completion delivered, then the queue drained again
guest.bridge.entry                        the bridge's synchronous part of entry (up to the first await)
guest.dispatch/env/context/auth/validate.<slot>/handler/response   the SDK's ledger inside the world (overlaps entry/settle)
```

Through 0.0.5 the engine lines read `invoke.eval / invoke.jobs /
outcome.eval / pending.eval`, one guest call per job and per read
(ADR-0018 replaced them).

What the semantics require is the fresh world (instantiate + reset) and the
ownership accounting; everything else is implementation and is fair game
for P8 (`docs/ROADMAP.md`).

## Disclosure

Every report names the host (`docs/measurements/2026-09-18-p5-p6-qualification.md`
describes VM 47), the pinning, what else ran on the machine (a soak sharing
the VM cost the 2026-09-19 hello run ≈1.7× throughput at c=16), the versions
of every runtime, and the commit of this repository.
