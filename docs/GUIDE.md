# Usai Developer Guide (v0 preview)

> Declare work. Declare resources. Write business logic. Run Usai.

This guide is enough to build a real application on the current runtime. The API will change before the first stable release; everything here is implemented and tested in this repository.

## 1. Install and run

Three ways to run Usai; the native binary is the first-class one.

```bash
# 1. native — Node ≥ 24 + pnpm. `pnpm dev` (= `pnpm usai dev`) runs the `usai`
#    binary at the project's SDK version: a `usai` on PATH at that version, else
#    the GitHub release asset (Linux x86_64/aarch64, macOS arm64), fetched once
#    into ~/.cache/usai/<version> and SHA-256 verified. Or install the binary
#    yourself from the releases page and call `usai` directly.
pnpm dlx @sakaladev/create-usai my-app && cd my-app && pnpm install
pnpm dev

# 2. Docker, nothing installed locally: the scaffold's compose.yaml runs the
#    `-dev` image with your source mounted (installs on first start, reloads on save)
docker compose up

# 3. ship it: the scaffold's Dockerfile builds in the `-dev` image and copies only
#    the artifact into the runtime image — no Node, no source, non-root, read-only ok
docker build -t my-app . && docker run --rm -p 3000:3000 my-app
```

The npm wrapper honours `USAI_BIN` (use exactly this binary), `USAI_CACHE_DIR`
/ `XDG_CACHE_HOME` (where fetched binaries live) and `USAI_RELEASE_BASE` (a
mirror of the release assets, for air-gapped or proxied networks). Other
platforms: build with `cargo build --release -p usai-cli` and set `USAI_BIN`,
or use Docker.

Images: `sakaladev/usai:X.Y.Z` (runtime: the binary, CA roots, user `usai`
10001, `ENTRYPOINT usai`, `CMD run --artifact /app/.usai/build --host 0.0.0.0
--port 3000`) and `sakaladev/usai:X.Y.Z-dev` (runtime + Node 24, pnpm, git;
`CMD dev --host 0.0.0.0`), both `linux/amd64` and `linux/arm64`, also on
`ghcr.io/<owner>/usai`. Both images *are* the `usai` command (`docker run
--rm sakaladev/usai:X.Y.Z --version`); use `--entrypoint sh` for a shell.
Docker is zero-install, not zero-cost: bind mounts on macOS/Windows can be
slow to notice changes. One tag `vX.Y.Z` publishes binaries, the npm packages
(`@sakaladev/usai`, `@sakaladev/create-usai`, via Trusted Publishing) and the
images at the same version — keep them equal; an artifact built by another
format is refused before serving with a message that says which versions
built it and what this runtime understands.

```bash
pnpm dlx @sakaladev/create-usai my-app      # or: node packages/create-usai/dist/cli.js my-app (from this repo)
cd my-app && pnpm install
usai dev                         # from this repo: cargo run -p usai-cli -- dev --root my-app
curl localhost:3000/hello/world
```

`usai dev` builds, serves, and on every change builds a **new revision**, activates it, and drains the previous one. A failed build keeps the previous revision serving.

## 2. The two questions

Every Usai application answers two questions:

1. **What work am I defining?** — a workload: HTTP request, task, cron, command, queue message, WebSocket connection, stream, or service.
2. **What must outlive that work?** — a resource: PostgreSQL, a runtime-local cache, configuration.

Each workload kind has a natural lifetime and the runtime gives it exactly that:

| Kind | Declared with | Lifetime |
|---|---|---|
| HTTP request | `http.get/post/put/patch/delete/raw` | one request |
| Task | `task` | one invocation |
| Cron | `cron` | one tick |
| Command | `command` | one `usai app <name>` run |
| Queue message | `queue.consume` | one message |
| Stream | `http.stream` | until the handler returns |
| WebSocket | `socket` | the connection |
| Service | `service` | while the revision is active |

Every unit of work runs in a **fresh execution world**: fresh globals, nothing inherited from the previous request. Anything that must persist is a **resource**.

The full API surface is the SDK's type declarations — `node_modules/@sakaladev/usai/dist/*.d.ts` (`http.ts`, `workloads.ts`, `resources.ts`, `queue.ts`, `connection.ts`, `env.ts`, `test.ts`); every option shown below is documented there.

## 3. Project layout

```text
my-app/
├── src/app.ts          # the application root: defineApp({...})
├── usai.config.ts      # maps structure (entry, migration/seeder globs); no semantics
├── package.json
└── tsconfig.json
```

Larger projects compose modules:

```ts
// src/billing/module.ts
export const billing = defineModule({
  name: "billing",
  workloads: [createInvoice, reconcile],
  resources: [db],
  migrations: "./src/billing/migrations/*.sql",   // root-relative
  seeders: "./src/billing/seeders/*.ts",
});
```

`usai inspect` shows the application exactly as the runtime understood it. `usai config` shows every effective setting and where it came from.

`usai.config.ts` is a **declaration**, evaluated in a capability-less world and cached by its source digest — not a Node script. `process.env`, `fs`, `require`, `import.meta` are not available in it (the error says so). Anything that differs per environment belongs to `env(...)` declarations in the application and to the runtime's environment, never to the config.

Types: `usai build` runs the project's own `tsc -p tsconfig.json --noEmit` (when `tsconfig.json` and `typescript` are present) and refuses an artifact with type errors (`--no-typecheck` opts out); `usai dev` runs the same check in the background after every rebuild and prints the diagnostics while the new revision already serves. Errors in a world carry stacks mapped to your source (`src/notes/pg.ts:41:12`) through the bundle's source map.

## 4. HTTP

```ts
import { defineApp, http, errors, auth } from "@sakaladev/usai";
import { z } from "zod";

const Params = z.object({ id: z.string().uuid() });
const User = z.object({ id: z.string().uuid(), name: z.string(), email: z.string().email() });

export const getUser = http.get("/users/:id", { params: Params, response: { 200: User } }, async (ctx) => {
  const user = await findUser(ctx.params.id);      // ctx.params is typed and parsed
  if (!user) throw errors.notFound("user not found");
  return user;                                     // encoded per the 200 contract
});

export const createUser = http.post("/users", { body: NewUser, response: { 201: User } }, async (ctx) =>
  http.created(await insertUser(ctx.body)),
);
```

- Any Standard Schema library works (Zod 4, Valibot, ArkType …). When the library can describe itself as JSON Schema (Zod 4 does), invalid input is rejected **before a world exists**; otherwise it is validated inside the world.
- Return a plain value for the default status, `http.created(...)` / `http.response(status, body, headers)` for explicit ones, `http.noContent()` for 204.
- Errors: `errors.notFound()`, `errors.conflict()`, `errors.custom(code, status, message)`. Unknown exceptions are sanitized to `internal` under `usai run`; `usai dev` returns the exception message and stack to the client (`expose_diagnostics`) — details always go to the logs. A response that does not match its declared contract is a `500 response_contract_violation`; the failing paths are in the log and, in dev, in `error.details.issues`.
- Declare what you throw and return: OpenAPI and `usai inspect` only know the statuses you put in `response: { … }` and the codes in `errors: [...]`; `errors.notFound()` in a handler does not add a 404 to the document by itself.
- Validation issues use JSON-pointer paths (`/tags/0`) wherever the check ran — at the boundary or in the world.
- A resource the workload did not declare (`ctx.resources["x"]` without `resources: [x]`) is a `500 resource_not_declared` that names the fix.
- Raw endpoints: `http.raw("/webhook", async (ctx) => http.rawResponse(200, "ok"))` — exact bytes via `ctx.request.bytes()`, no contracts, documented as opaque.
- Auth is a declared boundary: `const authed = auth.bearer({ resolve: async (ctx, token) => … })`, then `http.get("/me", { auth: authed }, async (ctx) => ctx.auth)`.

## 5. Tasks: owned or transferred

```ts
export const sendReceipt = task("send-receipt", { input: z.object({ orderId: z.string() }) }, async (ctx) => { … });

export const order = http.post("/orders", { body: Order }, async (ctx) => {
  const total = await ctx.tasks.invoke(priceOrder, ctx.body);     // owned: this request waits
  await ctx.tasks.dispatch(sendReceipt, { orderId: total.id });   // transferred: runs in its own world later
  return total;
});
```

A finite world that ends with live asynchronous work (a stray `setTimeout`, an un-awaited promise holding a timer) is a **lifecycle error**: the runtime cancels it and logs a diagnostic naming the fix. Dispatched tasks are **not durable** — a crash may lose them; use the queue for durability.

## 6. Cron and commands

```ts
export const cleanup = cron("cleanup", { schedule: "0 3 * * *", timeout: "5m", overlap: "skip" }, async (ctx) => { … });
export const reconcile = command("reconcile", async (ctx) => ({ args: ctx.args }));
```

```bash
usai cron run cleanup          # one tick, now, without waiting for the clock
usai app reconcile -- --dry-run
```

## 7. PostgreSQL

```ts
import { postgres, env, type PostgresHandle } from "@sakaladev/usai";
const db = postgres("main", { pool: { max: 16 } });   // URL from DATABASE_URL

type UserRow = { id: number; name: string };
export const listUsers = http.get("/users", { response: { 200: z.array(User) }, resources: [db] }, async (ctx) => {
  const sql = ctx.resources["main"] as PostgresHandle;
  return sql.query<UserRow>(`select id, name from users order by id`);   // rows are Record<string, unknown> unless you say otherwise
});

export default defineApp({ workloads: [listUsers], resources: [db], env: env({ DATABASE_URL: env.url() }) });
```

- `sql.query(text, params)` → rows; `sql.one(...)` → row or null; `sql.execute(...)` → affected count. Parameters are typed from the prepared statement (`$1::int`, uuid, jsonb, timestamptz, arrays …) and encoded by the runtime, so a string that is not a valid uuid/timestamp for its slot is an `invalid_param` error (500) from the handler's point of view — validate boundary input with the schema first (`z.string().uuid()`), and keep timestamps as the ISO-8601 strings the driver returns (a `::text` cast produces PostgreSQL's own format, which does not round-trip).
- `sql.transaction(async (tx) => { … })` pins one connection for the callback: `tx.query/one/execute` run in one transaction, committed when the callback returns, rolled back when it throws (the error is rethrown). A handler that returns with the transaction still open is a lifecycle error — the runtime rolls back on its behalf and says so.
- Each operation leases one pooled connection. A connection is reused only after a **terminal** outcome; cancellation waits for the server to confirm; anything ambiguous is quarantined and replaced. Session state is reset between worlds.
- TLS: put `sslmode=require` in the URL (`prefer` is the default, `disable` turns it off). The server certificate is **always verified** — against Mozilla's roots plus the PEM bundle in `postgres("main", { tls: { caFile } })` or the `PGSSLROOTCERT` environment variable (private CAs, managed-database roots). There is no encrypted-but-unverified mode; a certificate that does not verify fails **activation**, not the first request.
- Migrations: SQL files found by `usai.config.ts` includes and module globs, applied in file-name order by `usai db migrate`, recorded in `usai_migrations`, never run at startup. `usai db status` shows them. `usai build` copies them into the artifact (`.usai/build/migrations/`), so a production image applies them without a source tree: `usai db migrate --artifact /app/.usai/build` (the scaffold's image: `docker run --rm -e DATABASE_URL=… my-app db migrate --artifact /app/.usai/build`).
- Seeders: a file exporting `seeder({ resources: [db] }, async (ctx) => { … })`, run by `usai db seed [name]` in its own world.

## 8. Queue

```ts
export const orders = queue.consume("orders", { message: OrderEvent, concurrency: 8, retry: { maxAttempts: 5, backoff: "exponential" } }, async (ctx) => {
  await process(ctx.message);            // ctx.attempt tells you which try this is
});
// from any world:
await ctx.queue.publish("orders", { orderId });
```

The v0 queue lives in PostgreSQL (`usai_queue`). Delivery is at-least-once; retry is only what you declare; exhausted messages go to the `dead` state.

## 9. Streams and WebSockets

```ts
export const events = http.stream("/events", {}, async (ctx, stream) => {
  while (!ctx.signal.aborted) { await stream.event("tick", { at: Date.now() }); await ctx.sleep("1s"); }
});

export const chat = socket("/chat", { incoming: Incoming, outgoing: Outgoing }, {
  async open(ctx) { ctx.state["count"] = 0; },
  async message(ctx) { ctx.state["count"] += 1; await ctx.send({ echo: ctx.message.text }); },
  async close(ctx) { … },
});
```

`ctx.state` is connection-local and disappears with the connection. A stream world lives until its handler returns, not until headers are sent.

## 10. Services

```ts
export const ledgerSync = service("ledger-sync", { restart: { mode: "on-failure" } }, async (ctx) => {
  const state = new Map();
  while (!ctx.signal.aborted) { await reconcile(state); await ctx.sleep("5s"); }
});
```

A service world starts with the revision and stops when the revision drains: the signal fires, `ctx.sleep` returns, the loop exits. Finite worlds cannot see its state.

## 11. Outbound HTTP

```ts
import { httpClient, type HttpClientHandle } from "@sakaladev/usai";
const payments = httpClient("payments", { baseUrlEnv: "PAYMENTS_URL", bearerTokenEnv: "PAYMENTS_TOKEN", timeoutMs: 5000, maxConcurrent: 16 });

export const charge = http.post("/charge", { body: Charge, resources: [payments] }, async (ctx) => {
  const api = ctx.resources["payments"] as HttpClientHandle;
  const res = await api.fetch("/v1/charges", { method: "POST", json: ctx.body });
  if (!res.ok) throw errors.badGateway(`payments answered ${res.status}`);
  return res.json();
});
```

There is no global `fetch` in a world (calling it rejects with `fetch_not_available` and this advice). Outbound HTTP is a **declared resource**: the runtime owns the client (pool, TLS roots, redirects, timeouts) and every `fetch` is an operation owned by the world — cancelled with it, bounded by its deadline. `baseUrl`/`baseUrlEnv` pins the destination (another origin is `origin_refused`); without it the client may call any http(s) URL. `maxConcurrent` refuses (503) instead of queueing. Responses: `status`, `ok`, `headers`, `text()`, `json()`, `bytes()`; a non-2xx status is data, not an exception. The destination shows in `usai graph`, `usai inspect` and `/_usai/docs`, and its counters in `/_usai/status`.

## 12. What a world can use

A world is a bare JavaScript engine with exactly these globals, no more: `console`, `setTimeout`/`clearTimeout`/`setInterval`/`clearInterval`, `queueMicrotask`, `atob`/`btoa`, `TextEncoder`/`TextDecoder`, `URL`/`URLSearchParams`, `structuredClone`, `crypto` (below), plus the SDK's `ctx`. Put `"types": ["@sakaladev/usai/globals"]` in `tsconfig.json` (the scaffold does) instead of the `DOM` lib or `@types/node`, so the compiler knows the same set.

`crypto` is a WebCrypto subset with a clear lifetime story: `crypto.randomUUID()`, `crypto.getRandomValues(typedArray)` (32 bytes of host entropy per world, expanded with SHA-256 — never the image's state), `crypto.subtle.digest("SHA-256" | "SHA-384" | "SHA-512", data)`, and HMAC through `subtle.importKey("raw", key, { name: "HMAC", hash: "SHA-256" }, …)` + `subtle.sign`/`subtle.verify` (constant-time compare). Password hashing is `password.hash(plain)` / `password.verify(plain, hash)` from the SDK — Argon2id run by the host off the world's thread; store the returned PHC string.

Deliberately absent, and why:

- **`fetch`** — outbound HTTP is the `httpClient` resource (§11); the global only explains that.
- **`process`, `fs`, `require`** — there is no filesystem or process in a world; declare what you need as a resource, read configuration from `ctx.env`.
- **`crypto.subtle` beyond digests and HMAC** (RSA, ECDSA, AES) — not yet; ask with the use case.

Zod checks that need one of the absent globals fail inside the world for every input (that was the case for `z.string().url()` before `URL` was provided).

## 13. Configuration and environment

```ts
export default defineApp({
  …,
  env: env({ DATABASE_URL: env.url(), APP_ENV: env.enum(["development", "production"]), WORKERS: env.int(), DEBUG: env.optional(env.bool()) }),
});
```

Missing or malformed values fail **activation**, not the first request. Inside a world, `ctx.env.WORKERS` is a number (`ctx.env` is typed `Record<string, string | number | boolean | undefined>`; narrow per key, or `const e = ctx.env as EnvValues<typeof spec>`).

Where values come from: the process environment. For local work, `usai dev`, `usai test`, `usai db …`, `usai app/cron/task …` also read `<root>/.env` (`KEY=VALUE`, `#` comments, quotes; never overriding what the shell set); `usai dev` re-reads it on every rebuild, so editing `.env` and saving takes effect without a restart. `usai run` does **not** read `.env` — production configuration belongs to the deployment environment (its error says so when a variable is missing).

`cache.local` is local to **one runtime process**: `usai dev`'s server and a one-shot `usai app <cmd>` / `usai cron run` are different runtimes and do not see each other's cache. State that must be shared belongs in PostgreSQL.

Deployment settings (port, budgets, limits) are runtime flags and environment, not application code: `usai run --port 8080 --status`.

## 14. Operate

```bash
usai build                     # .usai/build/{manifest.json, app.js} + cache/image.cwasm (engine cache for this host; install loads it in ms, drop it and install compiles)
usai run --artifact .usai/build --port 8080 --status    # /_usai/status + /_usai/metrics + /_usai/docs + /_usai/openapi.json (all on in `usai dev`)
curl :8080/_usai/status        # runtime truth: gauges, revisions, services, tasks, resources
curl :8080/_usai/metrics       # Prometheus text
usai generate openapi --out openapi.json
usai graph                     # workload → resource / dispatch graph
usai bench --path /users -c 16 -d 30   # engineering load test
```

Ctrl-C drains in-flight work with a bound; a second Ctrl-C forces exit. Read `docs/THREAT-MODEL.md` before exposing anything.

Deploying with Docker: build the scaffold's `Dockerfile`, run it with
`DATABASE_URL` and the rest of the declared environment (`usai run` never
reads `.env`), `--read-only --tmpfs /tmp` works; `docker stop` sends SIGTERM,
which drains with the same bound as Ctrl-C. A compose file for app +
PostgreSQL is the commented block in the scaffold's `compose.yaml`.

Signed artifacts: `usai keygen` makes an Ed25519 key; `usai build --sign usai-signing.key` (or `USAI_SIGNING_KEY`) writes `signature.json` — the SHA-256 of every file in the artifact, the native `cache/image.cwasm` included, signed. A runtime started with `usai run --artifact … --require-signature <public key>` (or `USAI_REQUIRE_SIGNATURE`, a hex key or a file with one key per line) refuses, before listening, an unsigned artifact, one signed by an untrusted key, any changed file, and any file the signature does not cover; the control surface applies the same rule to installs. Keep the private key in CI's secret store and the public keys in the deployment's environment.

`/_usai/docs` is the API reference, generated from the definition the runtime is executing (the revision identity is on the page). Beyond parameters, bodies and responses it shows what each request *does*: which slots are refused before a world exists, the world's lifetime and effective deadline, the resources it leases per operation, the tasks it hands off — and the tasks, crons, commands, queues and services that are not HTTP but run beside them. Every operation has a copyable curl and a "Try it" panel that sends a real request to this server and reports the runtime's own time (`x-usai-server-ms`). Declare `dispatches(endpoint, task)` and your `errors`/`response` statuses so the page can say so. `/_usai/openapi.json` carries the same facts as `x-usai-*` extensions for other tools.

## 15. Testing

Tests run the same application model as production:

```ts
import { test } from "node:test";
import assert from "node:assert/strict";
import { testApp } from "@sakaladev/usai/test";

test("users", async () => {
  const app = await testApp({ root: "." });          // spawns the runtime for this project
  try {
    const res = await app.http.post("/users", { body: { name: "Ayu", email: "ayu@x.io" } });
    assert.equal(res.status, 201);
    const receipt = await app.task("send-receipt").invoke({ orderId: "o1" });   // fresh world, no queue
    assert.ok(receipt.ok);
    await app.cron("cleanup").run();                                            // one tick, no wall clock
    await app.command("reconcile").run(["--dry-run"]);
  } finally {
    await app.close();
  }
});
```

`testApp` needs the `usai` binary (`USAI_BIN` or on `PATH`) and the project's declared environment (pass `env: { DATABASE_URL }`). With `migrate: true` (or `migrate: { seed: true }`) it runs `usai db migrate` / `usai db seed` first — for a throwaway database. Lifecycle-specific tests are ordinary: mutate in one request, read in the next, and assert the mutation is gone.

## 16. A realistic application: `examples/todos`

Everything above in one project — read it in this order.

```text
examples/todos/
  usai.config.ts                  app entry + where migrations/seeders live (globs)
  src/app.ts                      defineApp: modules + typed env
  src/resources.ts                one PostgreSQL pool, shared by both modules
  src/todos/module.ts             HTTP CRUD, a cron, a command; dispatches a task
  src/todos/migrations/001_todos.sql
  src/todos/seeders/sample.ts
  src/activity/module.ts          the task that records activity in its own world
  src/activity/migrations/002_activity.sql
  test/todos.test.ts              usai/test: migrate + seed, HTTP, task, cron, command
```

1. **Resources first** (`src/resources.ts`): `postgres("main", { pool: { max: 8 } })`. The pool is runtime-lifetime; nothing in a handler owns a connection for longer than one operation.
2. **A module per concern** (`src/todos/module.ts`, `src/activity/module.ts`): `defineModule` groups workloads with the migrations and seeders that belong to them; `defineApp` composes modules and declares the env the whole application needs.
3. **Contracts at the boundary**: `POST /todos` declares `body: NewTodo` and `response: { 201: Todo }`; `GET /todos` declares a `query` with coercion and defaults. Invalid input is refused before any world exists — `usai inspect` shows `validated before world creation`.
4. **Work that outlives the request is transferred, not detached**: `create` calls `ctx.tasks.dispatch(record, …)` — the request answers now, the activity row is written by `record-activity` in a fresh world, and the runtime owns that hand-off. Forgetting the `await` on a bare promise instead would be reported as detached work.
5. **Scheduled and operator work are finite worlds too**: `cron("purge-completed", …)` and `command("stats", …)` run in fresh worlds; `usai cron run purge-completed` and `usai app stats` invoke them without a server.
6. **Migrations are a deploy step**: `usai db migrate` applies `001_todos.sql` then `002_activity.sql` (file-name order across modules) and records them; `usai db status` shows the ledger; the runtime never migrates at startup.
7. **Tests drive the real binary** (`test/todos.test.ts`): `testApp({ migrate: { seed: true } })` migrates and seeds a throwaway database, then exercises HTTP, the task, the cron tick and the command through the control surface.

```bash
export DATABASE_URL=postgres://usai@localhost:5432/todos      # a database you can throw away
usai db migrate --root examples/todos && usai db seed --root examples/todos
usai dev --root examples/todos
curl -X POST localhost:3000/todos -H 'content-type: application/json' -d '{"title":"read the guide"}'
usai app stats --root examples/todos
usai test --root examples/todos
```

## 17. The P4 application: `examples/invoicing`

The roadmap's "real application" gate: tenants, users and bearer sessions (`password.hash`, random tokens from `crypto`, only SHA-256 hashes stored), invoices with line items created in one `db.transaction`, keyset pagination, status transitions that answer 404/409 with the current state, tenant isolation, a daily cron, a command with arguments, a seeder, and **signed webhooks**: the `issue`/`pay`/`mark-overdue` workloads publish to a queue (`publishes(...)` records it for `usai graph`), the `webhook.deliver` consumer POSTs through an `httpClient` with an HMAC signature and retries five times with exponential backoff, recording every attempt. Its test spins a local sink that fails the first delivery, then checks the retry landed with a valid signature.

What building it as a user found (and what changed): outbound HTTP, `crypto`, password hashing and transactions did not exist (ADR-0017); PostgreSQL enum parameters and columns were unsupported (now labels); a webhook consumer needs the *generic* client (tenant-chosen URLs), which is why `httpClient` without a `baseUrl` exists at all.

## 18. Performance note (v0)

Per-world cost on the Wasm substrate is flat with respect to application size: instantiating a world from the pre-initialized image costs ~0.02 ms whatever the bundle contains, and validators declared as contracts are prepared before the image is snapshotted, so a fresh world does not rebuild them. On the research VM a contract-validated hello request is ~1 ms p50 at c=1 and the runtime serves ~13k req/s at c=16 on 16 cores; the numbers and their attribution are in `docs/measurements/`. Handler code runs in an interpreter compiled by Cranelift: CPU-heavy loops are slower than on a JIT; keep hot loops small or move them to the database.
