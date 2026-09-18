# Usai Developer Guide (v0 preview)

> Declare work. Declare resources. Write business logic. Run Usai.

This guide is enough to build a real application on the current runtime. The API will change before the first stable release; everything here is implemented and tested in this repository.

## 1. Install and run

Prerequisites: Node ≥ 24 and pnpm (or npm). The `usai` binary comes from a
GitHub release (`usai-vX.Y.Z-<target>.tar.gz` for Linux x86_64/aarch64 and
macOS arm64, with a SHA-256 next to it) — or from `cargo build --release -p
usai-cli` in this repository (Rust 1.98). Releases are cut by tagging
`vX.Y.Z` (`.github/workflows/release.yml`); the same tag publishes the
`usai` and `create-usai` packages to npm.

```bash
pnpm dlx create-usai my-app      # or: node packages/create-usai/dist/cli.js my-app (from this repo)
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

## 4. HTTP

```ts
import { defineApp, http, errors, auth } from "usai";
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
- Errors: `errors.notFound()`, `errors.conflict()`, `errors.custom(code, status, message)`. Unknown exceptions are sanitized to `internal`; details go to logs.
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
import { postgres, env, type PostgresHandle } from "usai";
const db = postgres("main", { pool: { max: 16 } });   // URL from DATABASE_URL

export const listUsers = http.get("/users", { resources: [db] }, async (ctx) => {
  const sql = ctx.resources["main"] as PostgresHandle;
  return sql.query(`select id, name from users order by id`);
});

export default defineApp({ workloads: [listUsers], resources: [db], env: env({ DATABASE_URL: env.url() }) });
```

- `sql.query(text, params)` → rows; `sql.one(...)` → row or null; `sql.execute(...)` → affected count. Parameters are typed from the prepared statement (`$1::int`, uuid, jsonb, timestamptz, arrays …).
- Each operation leases one pooled connection. A connection is reused only after a **terminal** outcome; cancellation waits for the server to confirm; anything ambiguous is quarantined and replaced. Session state is reset between worlds.
- TLS: put `sslmode=require` in the URL (`prefer` is the default, `disable` turns it off). The server certificate is **always verified** — against Mozilla's roots plus the PEM bundle in `postgres("main", { tls: { caFile } })` or the `PGSSLROOTCERT` environment variable (private CAs, managed-database roots). There is no encrypted-but-unverified mode; a certificate that does not verify fails **activation**, not the first request.
- Migrations: SQL files found by `usai.config.ts` includes and module globs, applied in file-name order by `usai db migrate`, recorded in `usai_migrations`, never run at startup. `usai db status` shows them.
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

## 11. Configuration and environment

```ts
export default defineApp({
  …,
  env: env({ DATABASE_URL: env.url(), APP_ENV: env.enum(["development", "production"]), WORKERS: env.int(), DEBUG: env.optional(env.bool()) }),
});
```

Missing or malformed values fail **activation**, not the first request. Inside a world, `ctx.env.WORKERS` is a number.

Deployment settings (port, budgets, limits) are runtime flags and environment, not application code: `usai run --port 8080 --status`.

## 12. Operate

```bash
usai build                     # .usai/build/{manifest.json, app.js}
usai run --artifact .usai/build --port 8080 --status
curl :8080/_usai/status        # runtime truth: gauges, revisions, services, tasks, resources
curl :8080/_usai/metrics       # Prometheus text
usai generate openapi --out openapi.json
usai graph                     # workload → resource / dispatch graph
usai bench --path /users -c 16 -d 30   # engineering load test
```

Ctrl-C drains in-flight work with a bound; a second Ctrl-C forces exit. Read `docs/THREAT-MODEL.md` before exposing anything.

## 13. Testing

Tests run the same application model as production:

```ts
import { test } from "node:test";
import assert from "node:assert/strict";
import { testApp } from "usai/test";

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

`testApp` needs the `usai` binary (`USAI_BIN` or on `PATH`) and the project's declared environment (pass `env: { DATABASE_URL }`). Lifecycle-specific tests are ordinary: mutate in one request, read in the next, and assert the mutation is gone.

## 14. Performance note (v0)

Every world evaluates the application module afresh. The SDK itself costs ~1 ms per world on the current substrate; a full Zod build adds ~5 ms because Zod initializes per world (measured: `zod` 6.6 ms/world, `zod/mini` 1.3 ms/world, SDK only 1.0 ms/world, release build). `zod/mini` is much cheaper but does not expose JSON Schema, so contracts are validated inside the world and OpenAPI is degraded for them. Pick per endpoint; keep validation libraries small; this cost is the engine substrate's, not the lifecycle model's, and is the top item on the roadmap (`docs/STATUS.md`).
