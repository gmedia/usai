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

## What a freeze does **not** require

- P9's levers (boundary encoding, idle decommit, world entry): all internal.
- The interpreter's cost (Q18): internal.
- Sakala integration (D15): a separate project, and never a runtime
  dependency.

## The decision to make

Three choices, in the order they are usually taken:

1. **Freeze now at 1.0**, with the five moving items above accepted as they
   stand. Fastest for adopters; commits us to the 0.0.9 log shape.
2. **Freeze after one quiet release** (0.1.0 → 1.0): ship what is moving,
   change nothing for one release, then freeze. Costs one release cycle and
   removes the two shapes that changed this week.
3. **Stay `0.x`** and say so louder: an honest "not yet" is better than a
   1.0 that has to break.

Whichever it is, `SUPPORTED.md` → Versioning is the sentence that has to
change, and `docs/ROADMAP.md`'s RC row is the gate that closes.
