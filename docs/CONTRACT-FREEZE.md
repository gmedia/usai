# What 1.0 would freeze

`SUPPORTED.md` says the quiet part today: *"`0.0.x` (now): contracts may
change between versions"*. That sentence is the reason nobody can build on
this yet, and removing it is a decision a maintainer makes — not a task a
gate can pass. This page is the inventory that decision needs: **every public
surface, what freezing it would commit us to, and what is still moving.**

Read it as a checklist, not as a plan. Nothing here is frozen until the
maintainer says so and `SUPPORTED.md` changes.

## The rule a freeze would adopt

Once 1.0 ships, within `1.x`:

- **Adding** is allowed: a new option with a default, a new workload kind, a
  new resource kind, a new CLI verb, a new metric.
- **Removing or changing the meaning** of anything below needs a major
  version, and a deprecation that ships at least one minor first.
- **The artifact stays loadable**: an artifact built by `1.x` runs on every
  later `1.y ≥ x`. The manifest carries its format version and the runtime
  refuses, before listening, what it cannot serve.

## The surfaces

| Surface | Where | State | What freezing commits us to |
|---|---|---|---|
| **Declarations** — `defineApp`, `defineModule`, `http.*`, `socket`, `task`, `cron`, `command`, `service`, `queue.consume`, `seeder`, `dispatches`, `publishes` | `packages/usai/src` | Stable since 0.0.5; option keys are now compile-checked (0.0.8) | Every option name and its default. Adding options is free; renaming one is a major |
| **The context** — `ctx.body/params/query/headers`, `ctx.auth`, `ctx.env`, `ctx.log`, `ctx.sleep`, `ctx.signal`, `ctx.requestId`, `ctx.resources`, `ctx.tasks`, `ctx.queue` | same | Stable; `ctx.resources` typing landed 0.0.6 | The shape a handler receives, per workload kind |
| **Resource handles** — `postgres` (`query`/`one`/`execute`/`transaction`), `httpClient` (`fetch`), `cache.local` | same | `readonly SqlParam[]` since 0.0.9; raw transaction control refused since 0.0.8 | The method names, their arguments, and the **error codes** they throw |
| **Helpers** — `errors`, `cookies`, `tokens`, `bytes`, `multipart`, `password`, `env` | same | Stable; `tokens` added 0.0.6 | Signatures and, for `errors`, the code→status mapping |
| **The test harness** — `usai/test`: `testApp`, `app.http/task/queue/command`, `app.logs`, `app.waitForLog`, `app.stream`, `app.socket`, `res.violations` | `packages/usai/src/test.ts` | Stable since 0.0.6 | Test files are user code: breaking this breaks their CI |
| **The error envelope** — `{ error: { code, message, details? } }` and the runtime's code list | `http/pipeline.rs`, OpenAPI | Stable; `internal` sanitization is part of it | Every code a client can branch on. Adding codes is allowed if they are declared |
| **HTTP semantics** — validation before the world, 400/401/404/405/413/415/426/499/503/504, `Allow`, `x-request-id`, catch-alls, route precedence | GUIDE §4 | Stable since 0.0.6 | Status codes for the cases above, and the order of the checks |
| **The artifact** — `manifest.json` + `app.js` + `migrations/`, and `signature.json` when signed | ADR-0005, ADR-0020 | Format version in the manifest; CI tests N/N−1 both ways plus the SDK axis (0.0.8) | That a `1.x` artifact loads on every later `1.y` |
| **`GUEST_ABI`** | `definition.rs`, `manifest.ts` | 1 since 0.0.1; ADR-0018 kept it at 1 | A bump refuses old artifacts loudly — allowed in a major, or in a minor only if the old ABI keeps working |
| **The CLI** — `dev`, `build`, `run`, `inspect`, `graph`, `test`, `app`, `task`, `cron`, `queue`, `db`, `generate`, `config`, `keygen`, `probe`, `bench` and their flags | `crates/usai-cli` | Verbs stable; `queue status/prune/prepare` added 0.0.9 | Flag names and their meanings; scripts and Dockerfiles depend on them |
| **Environment** — every `USAI_*` | `docs/ENVIRONMENT.md` | Stable | Names and accepted values (the boolean forms were fixed in 0.0.5) |
| **Operator surfaces** — `/_usai/status`, `/_usai/metrics`, `/_usai/live`, `/_usai/ready`, `/_usai/docs`, `/_usai/openapi.json` | `http/`, `observability.rs` | Status JSON and metric names stable; `usai_workload_cpu_seconds_total` added 0.0.9 | Metric names and labels (a dashboard is a contract), and the status document's field names |
| **The control API** | `docs/CONTROL-API.md` | Stable since 0.0.6 | Paths, bodies, status codes — an orchestrator depends on them |
| **The database** — `usai_migrations`, `usai_queue`, `usai_cron_ticks` | GUIDE §7 | `usai_queue` gained `request_id` and two indexes in 0.0.6; tooling in 0.0.9 | Column names an operator queries, and that a new version brings the schema forward without a manual step |
| **Log lines** | GUIDE §14 | **Moving**: `--log-format json` became one object per line with `fields` as an object in 0.0.9 | Only once the shape settles. A shipper's pipeline is a contract too |

## What is still moving, and would have to settle first

1. **The JSON log line** — its shape changed in 0.0.9 (`fields` from string to
   object, empty fields dropped). One more release of nobody complaining is
   the right amount of soak for it.
2. **`performance.now()`** — became a real monotonic clock in 0.0.9. Same.
3. **The queue's schema** — 0.0.6 added a column and two indexes; the
   upgrade path is now `usai queue prepare`, but a freeze means promising
   that a future version's schema change stays online-safe.
4. **`crypto.subtle`** — digests and HMAC only, by design (ADR-0017). If
   RSA/ECDSA/AES are coming, they should come *before* the freeze, since
   adding them later is allowed but the shape of the subset is easier to get
   right once.
5. **An explicit retry declaration for tasks and cron** (ADR-0014's
   consequence): only `queue.consume` has `retry` today. Adding it later is
   additive, so this does not block a freeze — but it is the most likely
   "why can't I" of the first year.
6. **Multi-core** (Q8) — the answer is "run replicas". A freeze does not
   depend on it, but it will shape what `1.x` can add without a major.
7. **What readiness means** (Q19, opened 2026-09-23) — `/_usai/ready` fails
   when any bound resource fails its probe, and a proxy removes an unready
   upstream, so a shared database outage takes out routes that never touch
   the database. `USAI_READY_REQUIRES_RESOURCES=0` turns the coupling off
   per deployment. Which of the two is the *default* is a contract an
   operator builds a health check on, and flipping it after a freeze is a
   breaking change to every deployment that did not set the variable. It
   should be settled first — by a deployment or two that has lived through
   an outage, not by argument.
8. **`/_usai/` on a listener that does not serve it** — 404 since 0.0.9,
   including when a status token is set (it used to be 401 for two of the
   six paths). Anything scripted against the old answer breaks, which is the
   reason to let it sit one release before promising it.
9. **A declared `timeout:` on a stream, a socket or a service** — accepted
   and silently dropped until 0.0.10, now honoured (there is still no
   *default* deadline for those kinds). An application that declared one and
   relied on it being ignored changes behaviour on upgrade. It is a fix, not
   a contract change — the OpenAPI document had published the value as
   authoritative all along — but it moved this week.
10. **What per-workload latency measures** — 0.0.10 records a stream's as its
   world's lifetime rather than the time to its head. Every dashboard and
   alert built on `usai_http_workload_request_seconds_*` for a streaming
   application reads differently after the upgrade; that is the point, but it
   is a number people build on.
11. **`maxBodyBytes` on a route** (0.0.10) — additive, and a cap rather than
   a raise, so it cannot loosen an operator's ceiling. It is on this list
   only because declaring it where there is no request body is now an
   **install-time refusal**: an artifact that did so would have been accepted
   before.

Items 9–11 landed on 2026-09-24 from the seventeenth fresh-eyes round. They
are the concrete argument for option 2 below rather than option 1: three
contract-adjacent shapes moved in a single day, found by one developer in
twenty-one minutes, and none of them had been exercised by anyone before.

## What a freeze does **not** require

- P9's levers (boundary encoding, idle decommit, world entry): all internal.
- The interpreter's cost (Q18): internal.
- Sakala integration (D15): a separate project, and never a runtime
  dependency.

## The decision to make

Three choices, in the order they are usually taken:

1. **Freeze now at 1.0**, with the eleven moving items above accepted as
   they stand. Fastest for adopters; commits us to the 0.0.9 log shape *and* to
   0.0.9's answer on what readiness means (item 7), which is the one most
   likely to be regretted: it was written before anyone had lived through a
   database outage behind a proxy with it.
2. **Freeze after one quiet release** (0.1.0 → 1.0): ship what is moving,
   change nothing for one release, then freeze. Costs one release cycle and
   removes the shapes that changed this week — the log line, the readiness
   default, the `/_usai/` 404 — from the list of things frozen before anyone
   ran them in anger. **This is the recommendation**: everything on the
   moving list is a week old, and nothing on it is blocking an adopter today,
   because `0.0.x` already says contracts move.
3. **Stay `0.x`** and say so louder: an honest "not yet" is better than a
   1.0 that has to break.

Whichever it is, `SUPPORTED.md` → Versioning is the sentence that has to
change, and `docs/ROADMAP.md`'s RC row is the gate that closes.
