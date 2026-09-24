# Usai Developer Guide (0.0.x)

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
#   `dlx` caches what it fetched, so if you scaffolded once before and the
#   template looks old, name the version: pnpm dlx @sakaladev/create-usai@latest
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
├── usai.config.ts      # defineConfig({ app: "./src/app.ts" }) — migrations default to ./src/**/migrations/*.sql + ./migrations/*.sql, seeders to ./src/**/seeders/*.ts (`usai config` prints the effective values and their sources)
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
  migrations: "./migrations/*.sql",              // relative to this file (or to the project root)
  seeders: "./seeders/*.ts",
});
```

**Where your code lives is up to you; how the runtime finds it is not.** A workload exists because `defineApp` can *reach* it through imports — nothing scans your files. Split the application the way you would in Express (`src/routes/*.ts`, `src/controllers/*.ts`, `src/services/*.ts`) and hand the exports to `defineApp`:

```ts
// src/app.ts
import { defineApp } from "@sakaladev/usai";
import * as books from "./routes/books.routes.ts";     // a barrel re-export works too
import { billing } from "./billing/module.ts";

export default defineApp({
  name: "bookstore",
  modules: [billing],                    // a module brings its own workloads and migrations
  workloads: [...Object.values(books)],  // or list them one by one
});
```

The rule has one consequence worth stating plainly: **auto-discovery cannot work.** `fs.readdirSync("./routes")`, a glob, `import.meta.glob`, `require.context` and a dynamic `import(\`./routes/${name}\`)` all fail at build — the application is bundled and its manifest is read by evaluating the module in a world that has no filesystem, before any work runs (top-level `await` is out for the same reason). A route file nobody imports is not served; a build that produced no workloads says so, and so does the runtime at activation.

### Routes, services, repositories

Handlers that hold business logic get ugly fast, and the layering most Express or Fastify teams end up with ports here unchanged — `routes.ts` declares, `service.ts` decides, `queries.ts` talks to the database, `schemas.ts` holds the contracts, `mapper.ts` shapes the response:

```text
src/users/
├── module.ts     # defineModule({ name: "users", workloads: [...], resources: [db] })
├── routes.ts     # http.post(...) — contracts, auth, resources; one line of body
├── service.ts    # the decisions: what is allowed, what conflicts, what to write
├── queries.ts    # the repository: SQL, and nothing else
├── schemas.ts    # the zod schemas the routes declare
├── mapper.ts     # row → response shape
└── migrations/*.sql
```

**The one rule the runtime adds: a repository takes its executor as an argument.** In Express you close over a pool created at module scope; here the handle exists only inside a world (and module scope is a snapshot, so a value written there never reaches the next world). Type the repository against `SqlExecutor` — the three methods — and the same function works on the handle *and* inside a transaction, because `tx` is an `SqlExecutor` too:

```ts
// queries.ts — the repository. `SqlExecutor` is query/one/execute; `PostgresHandle` adds transaction().
export async function insertUser(sql: SqlExecutor, email: string): Promise<UserRow> {
  const row = await sql.one<UserRow>(
    `insert into users (email) values ($1) returning id, email, created_at as "createdAt"`, [email]);
  if (!row) throw new Error("insert returned no row");
  return row;
}

// service.ts — the decisions. No HTTP in sight, and it composes the repository.
export async function register(sql: PostgresHandle, email: string): Promise<UserRow> {
  return sql.transaction(async (tx) => {
    const existing = await tx.one<{ id: string }>(`select id from users where email = $1`, [email]);
    if (existing) throw errors.conflict("that email is registered");
    return insertUser(tx, email);          // the same repository function, now on the transaction
  });
}

// routes.ts — the declaration, and one line of body.
export const create = http.post(
  "/users",
  { body: NewUser, response: { 201: UserResponse }, resources: [db], errors: [{ code: "conflict", status: 409 }] },
  async (ctx) => http.created(toUserResponse(await register(ctx.resources.main, ctx.body.email))),
);
```

Two habits that do not survive the move: a singleton built at module scope that holds a connection, a cache or a counter (worlds do not share it, and what a handler writes there is discarded), and a service that reaches for a global to find the database — pass it in, which is what makes the service testable anyway. Everything else — dependency direction, naming, one folder per concern — is yours.

`usai inspect` shows the application exactly as the runtime understood it. `usai config` shows every effective setting and where it came from.

`usai.config.ts` is a **declaration**, evaluated in a capability-less world and cached by its source digest — not a Node script. `process.env`, `fs`, `require`, `import.meta` are not available in it (the error says so). Anything that differs per environment belongs to `env(...)` declarations in the application and to the runtime's environment, never to the config.

Types: `usai build` runs the project's own `tsc -p tsconfig.json --noEmit` (when `tsconfig.json` and `typescript` are present) and **fails** on type errors (`--no-typecheck` opts out) — exit code 1 and "do not ship it"; the artifact it wrote is coherent, so a pipeline must check the exit code rather than assume the directory is untouched. A build that fails for any other reason leaves the **previous** artifact in place and runnable: everything fallible happens in a staging directory and the artifact is published by renaming into it. `usai dev` runs the type check in the background after every rebuild and prints the diagnostics while the new revision already serves. Errors a handler `throw`s carry stacks mapped to your source (`src/notes/pg.ts:41:12`) through the bundle's source map; an error raised by a **host operation** (a bad SQL parameter, an outbound call) is not thrown from your frames and carries the runtime's — which is why those messages name the statement or the operation instead.

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

- Any Standard Schema library works (Zod 4, Valibot, ArkType …). When the library can describe itself as JSON Schema (Zod 4 does), invalid input is rejected **before a world exists**; otherwise it is validated inside the world. The handler always receives the library's own output: a schema built from structural types, the library's own checks and `.default()` is validated once, at the boundary, and the world only strips undeclared keys and fills defaults the way Zod does; a schema that changes the value (`.transform()`, `.preprocess()`, `.catch()`, `.trim()`, coercion of a value the host did not already coerce) is parsed again in the world so it behaves exactly as the library defines it.
- Return a plain value for the default status, `http.created(...)` / `http.response(status, body, headers)` for explicit ones, `http.noContent()` for 204.
- Errors: `errors.notFound()`, `errors.conflict()`, `errors.custom(code, status, message)`. Unknown exceptions are sanitized to `internal` under `usai run`; `usai dev` returns the exception message and stack to the client (`expose_diagnostics`) — details always go to the logs. A response that does not match its declared contract is a `500 response_contract_violation`; the failing paths are in the log and, in dev, in `error.details.issues`.
- Declare what you throw and return: OpenAPI and `usai inspect` only know the statuses you put in `response: { … }` and the codes in `errors: [...]`; `errors.notFound()` in a handler does not add a 404 to the document by itself.
- `operationId: "listProducts"` on any endpoint names the operation for generated clients (derived from method and path otherwise, `getProducts`); a socket's `incoming`/`outgoing` contracts become `components.schemas.<OperationId>Incoming/Outgoing`, in the public profile too.
- **Stale clients.** A deployed SPA cannot see that the contract changed under it unless the server says so. Two ways, both already there: `defineApp({ headers: { "x-app-version": "1.4.2" } })` puts your release on every response (the client compares it with the version it was built against and reloads), and the public OpenAPI document's `info.version` is the artifact's identity — it changes with every build, so a client that fetched the document at startup can poll it. **It also changes when you upgrade the SDK, with identical source** — the SDK's own runtime code is bundled into `app.js`, so a new SDK really does produce a different artifact (the identity deliberately ignores `builtWith`, but not the code). It identifies the artifact, not your commit: to tie a running instance to a source revision, put your own version in `defineApp({ headers })` or compare the artifact directory you deployed, not the identity across versions. The runtime adds no marker of its own: a revision id is an operator fact (`/_usai/status`), not a client contract.
- Response headers a handler sets are invisible to the document unless declared: `responseHeaders: { 201: { location: "URL of the new user" }, "*": { etag: "Version of the resource" } }` (on `http.*`, `http.raw`, `http.stream`) lists them per status in the OpenAPI document (`responses[status].headers`, typed as strings) and in the reference; `"*"` means every success status. Descriptive — the runtime does not check them.
- Declare failures in `errors: [{ code, status }]`, not as schemas in `response:` — the document then types the status as the error envelope with `error.code` narrowed to the declared codes (`if (res.status === 409) res.error.code` is `"duplicate_sku"` in a generated client); a `401: Err` in `response` is described as a plain body. Every declared status carries its reason phrase (`Not Found`, `Locked`).
- Unknown keys in a body are **stripped** by default (`z.object`): a client's typo passes as if absent. `z.strictObject({ … })` (or `.strict()`) refuses them at the boundary — `400 validation_failed` naming the key — which is what a team that generates clients from the document usually wants.
- Validation issues use JSON-pointer paths (`/tags/0`) wherever the check ran — at the boundary or in the world. The *message* differs by where it ran: the boundary's messages are JSON-Schema-derived (`"abc" does not match the expected format`), the world's are the library's own (`Invalid UUID`); the `code`, the `slot` and the paths are the same.
- `ctx.resources` is typed from the workload's `resources: [...]`: `resources: [db]` with `const db = postgres("db")` makes `ctx.resources.db` a `PostgresHandle`, `cache.local` a `CacheLocalHandle`, `httpClient` an `HttpClientHandle`; an undeclared name is a compile error, and at runtime (`ctx.resources["x"]` without `resources: [x]`) a `500 resource_not_declared` that names the fix. An auth scheme's resolver is typed the same way from its own `auth.bearer({ resources: [db], resolve })` — nothing on this surface needs a cast, and an `as SomeHandle` in your code is a sign the workload forgot to declare the resource.
- Paths: `:id` declares a parameter; `*path` as the last segment is a **catch-all** that takes the rest of the path as one parameter (`/files/*path` → `ctx.params.path` = `a/b/c.txt`; declare it in `params` like any other). A literal segment always wins over a parameter at the same position (`/invoices/summary` beats `/invoices/:id`, whatever the declaration order), and a literal path wins over a catch-all — except for a method the literal does not have: `http.options("/*any", …)` answers the preflight for every path while `GET /users/:id` keeps serving GET. A catch-all never turns an unknown URL into a `405` for methods it lacks (that stays `404 route_not_found`). The same method and path twice fails the build.
- `summary` (one line) and `description` (a paragraph) on any workload's options feed the reference page and the OpenAPI document; `auth.bearer({ description })` documents the security scheme.
- Raw endpoints: `http.raw("/webhook", async (ctx) => http.rawResponse(200, "ok"))` — exact bytes via `ctx.request.bytes()`, no contracts, documented as opaque.
- Auth is a declared boundary: `const authed = auth.bearer({ resolve: async (ctx, token) => … })`, then `http.get("/me", { auth: authed }, async (ctx) => ctx.auth)`. Four schemes: `auth.bearer` (`Authorization: Bearer`), `auth.header({ header })` (an API key), `auth.cookie({ cookie })` (a session cookie), `auth.custom({ resolve, credential })` for anything else — the resolver reads `ctx.request` itself. A scheme declares **the resources its resolver leases** — `auth.bearer({ resources: [db], resolve: async (ctx, token) => ctx.resources.db.one(…) })` — typed on the resolver's `ctx.resources`, and every workload that uses the scheme gets them without listing them again — typed on the handler's `ctx.resources` too (`usai inspect` and the reference show them on the operation).
- **Cookies.** `const session = auth.cookie({ cookie: "sid", resolve: async (ctx, sid) => { const userId = await cookies.verify(sid, ctx.env.SESSION_SECRET); if (!userId) throw errors.unauthorized("log in again"); return { userId }; } })` — the resolver receives the cookie's value, a missing cookie is a 401 before the handler. Login sets it: `http.response(200, body, { "set-cookie": cookies.serialize("sid", await cookies.sign(user.id, secret), { maxAge: 86_400 }) })` (Secure, HttpOnly, SameSite=Lax are the defaults); logout clears it with `{ maxAge: 0 }`; several cookies at once are an array value (`"set-cookie": [a, b]`). `cookies.parse/serialize/sign/verify` are the helpers (HMAC-SHA256, base64url, constant-time verify, several secrets for rotation). The OpenAPI document says `apiKey in: cookie` and the reference's request panel sends the browser's cookie (`credentials: include`). A custom scheme that reads something else declares where with `credential: { in, name }`; without it the document says only that a custom resolver reads the request, and never invents an `Authorization` header a generated client would send for nothing.
- **Access tokens for API and mobile clients.** `tokens.sign({ sub: user.id }, ctx.env.ACCESS_TOKEN_SECRET, { expiresIn: "15m" })` makes a signed, self-describing bearer token (`<payload>.<signature>`, HMAC-SHA256, base64url, `iat`/`exp` added); the resolver checks it without a database hit — `const claims = await tokens.verify<{ sub: string }>(token, secret); if (!claims) throw errors.unauthorized();` — and `tokens.verify(token, ["new", "old"])` rotates. It is not a JWT: no header, no algorithm choice, no third party can verify it (a world has HMAC only, §12). Refresh tokens, sessions and "log out everywhere" are rows: store a hash of the refresh token with its family and revoke on reuse. Decide what a stateless token may not know — a deleted account, a changed role — and either accept it until `exp` (keep `expiresIn` short) or add one primary-key lookup in the resolver.
- **Lockouts and rate limits.** There is no counter primitive in the runtime (`cache.local` is per process and non-durable, so it cannot hold a lockout); a table is the right place and one statement does it: `insert into login_attempts (email, failed, locked_until) values ($1, 1, null) on conflict (email) do update set failed = login_attempts.failed + 1, locked_until = case when login_attempts.failed + 1 >= 5 then now() + interval '15 minutes' end returning locked_until`, answered with a declared `423 account_locked` (per account) — `429` stays for a per-client limit. Per-IP limits belong to the proxy in front (§14): a world sees the client's address only as the `x-forwarded-for` header the proxy sets, `ctx.headers["x-forwarded-for"]`, and only when there *is* a proxy — do not trust it otherwise.
- **Deadline.** Every finite workload has one: `timeout: "5s"` in its options, **30 s by default**. At the deadline the world is cancelled — a query in flight is cancelled on the PostgreSQL server and its connection quarantined until the server confirms (C5), an open transaction rolls back — and the client gets `504 deadline_exceeded`, with a `WARN deadline exceeded` line carrying the workload, the request id, `duration_ms` and `cpu_us` (so the answer to "which request timed out last night, and was it waiting or computing" is in the ordinary log). **A connection-bound or persistent workload — a stream, a socket, a service — gets no deadline by default**, because one that ended after 30 s would be useless; a `timeout:` it *declares* is honoured, ends the world at that point, and for a stream means the client's body stops there (counted in `usai_http_streams_failed_total`, with a WARN that says it was the declared timeout and not a failure). `usai inspect` prints the effective deadline for every workload and where it came from; so does the reference.
- **Body size.** The runtime reads at most `USAI_MAX_BODY_BYTES` (1 MiB by default) per request and answers `413 payload_too_large` above it, before any world exists. That variable is the **process** ceiling; a route declares `maxBodyBytes:` in its options to accept **less**, never more, and the effective bound is the smaller of the two. So the shape to reach for is: raise the process bound for the one route that takes uploads, and give every other route a small bound of its own — the equivalent of `express.json({ limit })` per route, with the operator still holding the ceiling. The 413 message says which of the two refused the request. Declaring it on a workload with no request body (a task, a cron tick, a stream, a socket) is refused at install rather than ignored.
- **Uploads.** A file arrives on an `http.raw` route as the exact bytes (`ctx.request.bytes()`); `multipart.parse(bytes, ctx.headers["content-type"])` splits a `multipart/form-data` body into `fields` and `files` (`{ name, filename, contentType, data }`) and `multipart.encode(parts)` builds one for a test (`app.http.post(path, { body, headers: { "content-type": contentType } })`) or an outbound call, and a `Uint8Array` goes straight into a `bytea` column. The body bound applies (`USAI_MAX_BODY_BYTES`); anything larger than a form attachment is better uploaded by the client straight to object storage with a presigned URL the application mints (HMAC-SHA256 is in `crypto.subtle`). A raw route cannot declare its request content type; the document lists it as opaque.

An auth `resolve` runs **inside the request's world** with the workload's declared `resources` and `env` (`ctx.resources["main"]` works there — ADR-0004): a session lookup is one query, and `usai inspect` shows `auth: <name> (resolved in world)`. The order follows from that: **boundary validation comes first**, then the world, then the resolver — so an unauthenticated request with an invalid body gets the `400` (with its field-level issues), not a `401`, and only a request that passed its contracts costs a world. Both the missing credential (the SDK answers `unauthorized` before calling your resolver) and the wrong one (your resolver's `errors.unauthorized`) are decided in the world, so a `401` costs a world where a `400` does not. If the shape of a contract must not be visible to anonymous callers, put it behind a route whose contract is opaque (a raw endpoint) or accept that validation is public, as it is on any schema-first API.

Raw endpoints (`http.raw`, exact bytes in — webhooks with signed bodies) can still declare `errors` and `responses: { 202: "accepted", 401: "bad signature" }` so the reference lists what the handler answers.

## 5. Tasks: owned or transferred

```ts
export const sendReceipt = task("send-receipt", { input: z.object({ orderId: z.string() }) }, async (ctx) => { … });

export const order = http.post("/orders", { body: Order }, async (ctx) => {
  const total = await ctx.tasks.invoke(priceOrder, ctx.body);     // owned: this request waits
  await ctx.tasks.dispatch(sendReceipt, { orderId: total.id });   // transferred: runs in its own world later
  return total;
});
```

A finite world that ends with live asynchronous work (a stray `setTimeout`, an un-awaited promise holding a timer) is a **lifecycle error**: the runtime cancels it and logs a diagnostic naming the fix. What the client sees depends on what was live: a pending timer or promise is cancelled and the response the handler returned is **committed as returned** (the violation is a log line, the `x-usai-lifecycle: detached_work` header under `--diagnostics`, and `res.violations` in tests); an **operation already started** — a query, a fetch, a publish — whose result the handler never awaited is a `500 detached_work`, because the runtime cannot say whether the side effect happened. Dispatched tasks are **not durable** — a crash may lose them; use the queue for durability. **At a drain** (a revision replacement, a stop), every background world of the revision — dispatched tasks, cron ticks, queue messages — gets the stop first, and **no new background work starts**: the cron scheduler and the queue consumers stop at the very beginning of the drain, before the grace period rather than after it, because the grace exists for a load balancer and for requests already in flight and nothing is watching a consumer. What has already been claimed finishes; nothing more is claimed by an instance that is leaving. For the worlds that are running: `ctx.signal` aborts, a pending `ctx.sleep` returns, operations keep running; a long task that checks `ctx.signal.aborted` between steps records where it got to and returns, and the drain finishes when it does. What has not returned by the drain bound (`--drain-timeout`, 30 s) is cancelled outright: no more code runs, the log says `task failed … termination="cancelled: …"`, and a queue message goes back for another attempt. A hard stop (SIGKILL) runs nothing.

## 6. Cron and commands

```ts
export const cleanup = cron("cleanup", { schedule: "0 3 * * *", timeout: "5m", overlap: "skip" }, async (ctx) => { … });
export const digest = cron("digest", { schedule: "0 7 * * *", exclusive: true, resources: [db] }, async (ctx) => { … });   // one instance per tick, however many replicas
export const reconcile = command("reconcile", async (ctx) => ({ args: ctx.args }));
```

`exclusive: true` is for the deployment with replicas: every instance's scheduler reaches the tick, each tries to insert `(schedule, scheduled time)` into `usai_cron_ticks` on the application's first `postgres` resource (or the one named in `exclusive: { database }`), and only the one whose insert landed runs the handler; the others count it under `cron.taken` in `/_usai/status`. No leader, no election, no clock agreement beyond the schedule itself. If the database is unreachable at the tick, every instance runs it (a flaky database must not silence a schedule everywhere at once) — write exclusive jobs to tolerate the rare double run. An exclusive schedule without a postgres resource is refused at install.

**A tick that had no scheduler alive is lost, not replayed.** The scheduler asks for the *next* occurrence after "now" each time round; nothing records that an earlier one was due. So a window with no instance scheduling — a `Recreate` rollout of a single cron replica, a crash, a scale-to-zero — simply has no ticks. That is the cron contract (`GOAL.md` §18: scheduled work is finite work the runtime starts, not a durable job store): if the work must survive such a gap, publish it to a queue and let a consumer catch up, or make the handler compute what it missed from the data (`… where processed_at is null`) rather than trusting that it ran.

```bash
usai cron run cleanup          # one tick, now, without waiting for the clock
usai app reconcile -- --dry-run
```

## 7. PostgreSQL

```ts
import { postgres, env } from "@sakaladev/usai";
const db = postgres("db", { pool: { max: 16 } });   // URL from DATABASE_URL

type UserRow = { id: number; name: string };
export const listUsers = http.get("/users", { response: { 200: z.array(User) }, resources: [db] }, async (ctx) => {
  return ctx.resources.db.query<UserRow>(`select id, name from users order by id`);   // typed from `resources: [db]`; rows are Record<string, unknown> unless you say otherwise
});

export default defineApp({ workloads: [listUsers], resources: [db], env: env({ DATABASE_URL: env.url() }) });
```

- **Row types.** Rows come back as `Record<string, unknown>`; a typed response contract makes the compiler ask for more. Say what a row is once, from the schema you already have: `type TaskRow = z.infer<typeof Task>; sql.one<TaskRow>(…)`. Column aliases (`total_cents::int as "totalCents"`) shape the row to the contract. `timestamptz`/`timestamp`/`date` columns arrive as ISO-8601 strings (`2026-09-19T05:53:01.781761+00:00`) — do **not** cast them `::text`, which yields PostgreSQL's own form (`2026-09-19 12:53:01.78+07`) that JavaScript's `Date` does not parse reliably. `uuid`, `numeric`, enums and arrays arrive as strings, strings, strings and JSON arrays respectively. Response bodies are serialised with keys in sorted order, whatever order the query selected them in.
- **Binary columns.** A `Uint8Array` parameter binds to `bytea` (`insert into attachments (data) values ($1)`, `[bytes]`); a `bytea` column comes back as a base64 string — `bytes.fromBase64(row.data)` is the `Uint8Array` again (`import { bytes } from "@sakaladev/usai"`). Fine for attachments of a few MB; a media store belongs in object storage.
- **Errors from SQL** arrive as `UsaiOperationError` with `err.usai.code` = `sql_<SQLSTATE>` (`sql_23505` for a unique violation, `sql_23503` foreign key, `sql_40001` serialization failure) and the server's message; connection loss is `connection_closed`; a world that waits more than `pool.acquireTimeoutSeconds` (10 s) for a connection gets `resource_exhausted`. Catch by code: `if (isUsaiError(e) && e.usai.code === "sql_23505") throw errors.conflict("email taken")`.
- `sql.query(text, params)` → rows; `sql.one(...)` → row **or `null`** (so `insert … returning id` needs `row!.id` or a check — the type does not know your statement returns one); `sql.execute(...)` → affected count. The parameter list is `readonly SqlParam[]` (`SqlParam` is exported): declare a built-up array as `const params: SqlParam[] = […]` and no cast is needed anywhere. Parameters are typed from the prepared statement (`$1::int`, uuid, jsonb, timestamptz, arrays, enums …) and encoded by the runtime; every other type (`interval`, `inet`, ranges, domains …) takes a string in its text form, parsed server-side like `'30 days'::interval`. A string that is not a valid uuid/timestamp for its slot is an `invalid_param` error (500) from the handler's point of view — validate boundary input with the schema first (`z.string().uuid()`). Timestamps accept ISO-8601 (including a bare calendar date — `2026-01-01` is midnight **UTC**, not the session's zone) and PostgreSQL's own text output. An `invalid_param` error names the statement it came from, because an error raised by a host operation carries the runtime's frames and not your handler's.
- `sql.transaction(async (tx) => { … })` pins one connection for the callback: `tx.query/one/execute` run in one transaction, committed when the callback returns, rolled back when it throws (the error is rethrown). A handler that returns with the transaction still open is a lifecycle error — the runtime rolls back on its behalf and says so. **Decide inside, throw outside**: a callback that writes a verdict (revoke the token family, count the failed attempt, mark the code used) and then throws `errors.unauthorized()` rolls the verdict back with the error. Return the verdict from the callback and throw after it commits — `const verdict = await sql.transaction(async (tx) => { …; return "reused"; }); if (verdict === "reused") throw errors.custom("refresh_token_reused", 401, …);` — the pattern every auth flow needs (lockout counters, reuse detection, single-use codes).
- **`begin`/`commit`/`rollback`/`savepoint`/`set` are refused on `query/one/execute`** and belong to `transaction(...)`. Each of those statements leases its own connection, so a raw `begin` would open a transaction on a connection the next statement does not hold — and return it to the pool in that state. The runtime answers `transaction_control` and names the helper instead of letting it half-work.
- Each operation leases one pooled connection. A connection is reused only after a **terminal** outcome; cancellation waits for the server to confirm; anything ambiguous is quarantined and replaced. Session state is reset between worlds.
- TLS: put `sslmode=require` in the URL (`prefer` is the default, `disable` turns it off). The server certificate is **always verified** — against Mozilla's roots plus the PEM bundle in `postgres("main", { tls: { caFile } })` or the `PGSSLROOTCERT` environment variable (private CAs, managed-database roots). There is no encrypted-but-unverified mode; a certificate that does not verify fails **activation**, not the first request.
- Connect timeout: `connect_timeout=<s>` in the URL (PostgreSQL's parameter); 5 s when absent. It matters for an *unreachable* server (a partition drops SYNs; a stopped one refuses at once): every request that needs the database holds its world for that long before its 503 — set it low for a service that must fail fast (`docs/runbooks/postgres-down.md`).
- Migrations: SQL files found by `usai.config.ts` includes and module globs, applied in file-name order by `usai db migrate`, recorded in `usai_migrations`, never run at startup. `usai db status` shows them. `usai build` copies them into the artifact (`.usai/build/migrations/`), so a production image applies them without a source tree: `usai db migrate --artifact /app/.usai/build` (the scaffold's image: `docker run --rm -e DATABASE_URL=… my-app db migrate --artifact /app/.usai/build`).
- Seeders: a file exporting `seeder({ resources: [db] }, async (ctx) => { … })`, run by `usai db seed [name]` in its own world; its return value is printed like a command's. Seeders are **not tracked** (unlike migrations there is no ledger; running one twice runs it twice — write `insert … on conflict do nothing`) and are **not part of the artifact** (they are source, built on demand): seed from a checkout or from the dev image, not from the runtime image. A module glob that matches no file is a WARN (`globs are root-relative`).

**Tables the runtime owns** in your database: `usai_migrations` (the ledger, written by `usai db migrate`), `usai_queue` (created on first use by a consumer or a publish, its columns added idempotently on upgrade — `request_id` in 0.0.6; `usai_queue_ready`/`_claim`/`_processing` indexes) and `usai_cron_ticks` (created on the first tick of an `exclusive` schedule). They are plain tables in the `public` schema of the resource's database — back them up with the rest, never edit `usai_migrations` by hand, and prune `usai_queue` rows in `done`/`dead` on your own schedule (the runtime never deletes them). **`usai_cron_ticks` is the exception: it prunes itself**, dropping that schedule's claims older than seven days on each successful claim, so a minute-by-minute exclusive schedule holds about ten thousand rows and not a year's worth. Nothing to do, and nothing to alert on. **`usai queue prune --state done --older-than 30d --yes`** is the supported verb — it ages a row by `created_at`, **counts unless you pass `--yes`**, and never runs by itself; `usai queue status` shows the depth per topic and state first. The equivalent `delete from usai_queue where …` works too, and `docs/runbooks/queue-dead-letter.md` has both.

**What the driver covers** (probed 2026-09-20 against PostgreSQL 18 through `sql.*`; everything is a prepared statement on a pooled connection, results in binary format):

| Works | Note |
|---|---|
| Parameters: text, int2/4/8, float, numeric (as string), bool, uuid, json/jsonb (as values), timestamptz/timestamp/date (ISO strings), `bytea` (`Uint8Array`), text[]/int4[]/int8[], enums by label | anything else (`interval`, `inet`, ranges, `money`, composites, domains) is sent in **text form** from a string and parsed by the server as `'…'::type` would |
| Results: the same types back; `numeric` as a string; `bigint` as a number within ±2^53 and **as a string beyond it** (a JSON number would round it); `bytea` as base64 | a result column of an exotic type fails with *unsupported column type; cast it in SQL* — `select started - finished as elapsed::text`, `ip::text`, `range::text` |
| `sql.transaction`: `SET LOCAL`, savepoints (`savepoint s1` / `rollback to savepoint s1`), `DECLARE … CURSOR` + `FETCH`, temp tables (`on commit drop`), advisory locks (`pg_try_advisory_xact_lock`) | all statements on the pinned connection; `SET LOCAL` and xact-scoped locks end with the transaction, which is what you want on a pool |
| `statement_timeout` | arrives as `sql_57014`; the world's own deadline cancels the query server-side the same way (C5) |
| One statement per call | `select 1; select 2` is `sql_42601` (prepared statements take one command) — two calls, or one transaction |

| Does not work | Instead |
|---|---|
| `COPY … TO STDOUT` / `FROM STDIN` | no COPY protocol on the wire (`postgres_error: unexpected message from server`); `insert … select` / `unnest($1::text[])` for bulk rows, or `psql \copy` from a job outside the runtime. **Arrays bind as `text[]`, `int4[]` or `int8[]` only**, so a bulk column of any other type goes in as `text[]` and is cast in the statement: `select i, c, t::timestamptz from unnest($1::int8[], $2::text[], $3::text[]) as u(i, c, t)`. Measured: 50 000 rows in ≈0.5 s of SQL, which is why the absence of COPY costs little at this scale |
| `LISTEN` / `NOTIFY` as a delivery path | `listen` is accepted but nothing reads the notifications — and it pins them to a pooled connection nobody owns; use the queue (§8) for work, or poll |
| Session-level state across calls (`SET` without `LOCAL`, session advisory locks, `PREPARE`) | every call leases a connection and gives it back — only `sql.transaction` keeps one; session state would leak to the next lessee |
| A `Uint8Array` result | `bytea` arrives as base64 (`bytes.fromBase64`) |

### Query builders, and what you can npm-install

There is no ORM, and there is a rule that decides every package: **a library that only computes is fine; one that reaches for a Node built-in when it is loaded is not** (a world has no `fs`, `net`, `events` or `process`, and the bundler says so by name). A connection pool, a driver and an ORM that opens its own connection are out — the database is the runtime's to lease. A query *builder* used as a compiler is in, and that turns out to be most of what people want from one:

```ts
// kysely as a compiler: DummyDriver never connects, .compile() gives { sql, parameters }
import { DummyDriver, Kysely, PostgresAdapter, PostgresIntrospector, PostgresQueryCompiler } from "kysely";
interface DB { books: { id: string; title: string; price_cents: number } }
const q = new Kysely<DB>({
  dialect: {
    createAdapter: () => new PostgresAdapter(),
    createDriver: () => new DummyDriver(),
    createIntrospector: (d) => new PostgresIntrospector(d),
    createQueryCompiler: () => new PostgresQueryCompiler(),
  },
});

export const list = http.get("/books", { query: Filter, resources: [db] }, async (ctx) => {
  const { sql, parameters } = q
    .selectFrom("books").select(["title"]).where("price_cents", ">=", ctx.query.minPrice)
    .compile();
  return ctx.resources.db.query<{ title: string }>(sql, parameters);
});
```

`drizzle-orm`'s `new QueryBuilder()` from `drizzle-orm/pg-core` works the same way (`.toSQL()` → `{ sql, params }`). Both were built and served real rows through `ctx.resources.db` on 0.0.7. **`knex` cannot be used at all**, in any configuration: it imports `events`, `util`, `fs` and friends at module load, so even the compile-only form fails to bundle.

What you keep by compiling rather than connecting: typed, composable query building *and* the runtime's leased connections, terminal-proof reuse and quarantine. What you give up: the library's own migrations, connection pooling and result mapping — migrations are `.sql` files here (`usai db migrate`), and rows come back as plain objects.

## 8. Queue

```ts
export const orders = queue.consume("orders", { message: OrderEvent, concurrency: 8, retry: { maxAttempts: 5, backoff: "exponential" } }, async (ctx) => {
  await process(ctx.message);            // ctx.attempt tells you which try this is
});
// from any world:
await ctx.queue.publish("orders", { orderId });
```

The v0 queue lives in PostgreSQL (`usai_queue`, created by the runtime in the consumer's `database` — by default the application's first `postgres` resource, which is also where `publish` writes; the publishing workload need not declare that resource). Delivery is at-least-once; retry is only what you declare; exhausted messages go to the `dead` state. A message must satisfy the consumer's `message` schema or it is dead-lettered on arrival — to add an event to an existing topic, extend the consumer's schema first, then publish it. Wrap the publisher in `publishes(workload, "orders")` so the reference links producer and consumer — and so the runtime can tell you at activation (and `usai inspect`) when a declared topic has **no consumer in this application**: a typo there is otherwise a silent sink (the row sits `ready` forever). Publishing to a topic another application consumes on the same database is legitimate; the warning is a warning.

Three things a queue user from another stack should know:

- **Throughput.** The queue costs one synchronous commit per message on your side (the handler's writes) plus the runtime's marks, which commit without waiting for the disk (a lost mark redelivers the message — at-least-once already allows that). So the rate is your database's commit latency: ≈45–50 messages per second per consumer worker on a 12 ms-fsync disk, ≈10× that on NVMe, linear in `concurrency` and in instances (measured: 712 msg/s at `concurrency: 16` on one instance, 956 on two — `docs/measurements/2026-09-18-p5-p6-qualification.md` → Queue throughput). A publish is one synchronous commit too (≈1 000–1 600/s from one world with 32 in flight; ≈100/s from a loop that awaits each).
- **Every runtime on the same database is a consumer.** Consumers claim with `SKIP LOCKED`, so a `usai dev` left running against the database your tests use will consume your tests' messages (and hand off *its* tasks with *its* environment). Give tests their own database (`testApp({ env: { DATABASE_URL } })`, `migrate: true`) or stop the dev server first.
- **`dispatch` is not a queue.** A dispatched task that fails is a `WARN task failed` and the work is gone; nothing retries. If the work must reach a third party (a merchant callback, an e-mail), publish it to a topic and consume it there — retry and dead-lettering are the queue's, not the task's.
- **Retention is yours.** Rows in `done` and `dead` stay until you delete them (`delete from usai_queue where state = 'done' and created_at < now() - interval '7 days'` from a cron is the usual shape); the runtime never purges.

A consumer's outcome is recorded in `usai_queue` after the handler returns (`done`, or `ready` again with the delay, or `dead`). That write is retried once if it fails and then logged (`could not record the message's outcome …`): until it lands the row stays `processing`, and the lost-consumer sweep redelivers it — so **a handler that finished can be redelivered**, which is the other half of at-least-once. With the default `retry.maxAttempts` of 1 there is no second attempt, so such a message is dead-lettered with `consumer lost: …` even though the work was done; declare `retry: { maxAttempts: 3 }` on anything you would rather see retried, and make the handler idempotent either way.

Idempotency: the same message can arrive twice (at-least-once, or two publishers). An `insert … on conflict do nothing` on an idempotency key, or a unique index and a `sql_23505` check, is simpler than a read-then-write inside the transaction. **Record the effect after it happened, not before you attempt it.** Claiming the key first and then doing the work looks right and drops jobs: the attempt that claimed it can die between the claim and the effect — a drain cancelling an outbound call, a `kill -9`, a deadline — and the retry then finds the claim, decides the work is done and marks the message `done` with the email never sent. Measured on this runtime under both failure modes. The safe shapes are to write the key **in the same transaction as the effect** when the effect is a database write, or, when the effect is an outbound call, to record *attempted* before and *succeeded* after and let the retry re-attempt anything that only reached the first — an at-least-once system needs the duplicate to be harmless, not the attempt to be unique. `usai queue run <topic> --message '{…}'` and `app.queue("<topic>").deliver(message)` in tests deliver one message to the consumer directly — a fresh world, `attempt` 1, no row, no retry — which is how to test that logic (deliver the same message twice).

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

**Fan-out: "A changes something, B's screen updates".** This is the question every realtime feature is, and the model's answer is not the one you bring from Node. There is no pub/sub, no broadcast, no cross-world push, and no `LISTEN`/`NOTIFY`: a world holds nothing another world can see, so one connection cannot reach another **except through a resource**. In practice that means a shared table and a poll per connection:

```ts
export const live = socket("/live/:board", { outgoing: Event }, {
  async open(ctx) {
    const db = ctx.resources["db"] as PostgresHandle;
    const board = ctx.params["board"] ?? "";
    await db.execute("insert into presence (board, client_id) values ($1, $2)", [board, ctx.requestId]);
    let seq = Number(ctx.query["since"] ?? 0);
    while (!ctx.signal.aborted) {
      const rows = await db.query("select seq, body from events where board = $1 and seq > $2 order by seq", [board, seq]);
      for (const row of rows) { await ctx.send(row.body as Event); seq = Number(row.seq); }
      await ctx.sleep("200ms");
    }
  },
  async close(ctx) {
    const db = ctx.resources["db"] as PostgresHandle;
    await db.execute("delete from presence where client_id = $1", [ctx.requestId]);
  },
});
```

**What that costs, and what it buys.** Each connection is one world for its whole life and one poll per interval: measured, 13 connections at 200 ms cost **≈4.8 queries per second per idle viewer**, so 2 000 viewers is 10 000 q/s through a pool of 16 — size the interval against the pool, not against how fresh the screen feels, and consider a coarser tick with a cheap `max(seq)` probe before the real read. What it buys is the thing a Socket.IO deployment fights for: **the fan-out crosses replicas by itself and needs no sticky sessions.** An event written on replica B reaches a client connected to replica A (measured at ≈1.3 s with a 200 ms poll and no configuration), presence is counted cluster-wide, and there is no Redis, no adapter and no `ip_hash` to get right. If the update rate is low and the screen is one-way, **SSE down and `POST` up is simpler still** (`http.stream` + `last-event-id` resumes for you).

**`open` must return before a message is delivered.** The socket world is one world: it runs `open`, then receives. A push loop in `open` is a *supported* shape — the loop above is the documented one — but while it runs, `message` does not fire and inbound frames wait. So a socket either **pushes** or **converses**, not both; for a screen that also sends, put the client→server direction on an ordinary HTTP route. When the client goes away, `ctx.signal` aborts, `ctx.sleep` returns, the loop ends and **`close` runs** — including when the loop ends because `ctx.send` rejected with `client_gone`, which is how a push loop normally ends. A client leaving is logged at debug, not as a failed handler.

**Heartbeats and the idle timeout.** A connection with no frame in **either** direction for `USAI_SOCKET_IDLE_TIMEOUT` (300 s) is closed with `1008 idle timeout` — server→client frames reset it too, so a busy board never idles out and a quiet one disconnects every viewer unless something ticks. A browser's `WebSocket` **cannot send a ping frame from JavaScript**, so "the client should ping" is not advice a browser can take: either have the server send a periodic tick (which a push loop already does), or have the client send an ordinary application message on a timer — and remember that a socket whose `open` is looping will not process that message until the loop ends. Note the consequence for a vanished client: a push loop resets its own idle timer with every push, so the idle timeout is not what reaps it — the abort above is.

**A socket's upgrade is validated like any other request.** `socket("/live/:board", { params: z.object({ board: z.string().uuid() }), query: … }, …)` checks both before the world exists: a bad one is a `400` naming the slot, and no connection is upgraded. Without the declarations `ctx.params` is a plain `Record<string, string>` the handler must check itself.

**Sizing.** Every open connection is one world for its whole life, out of the same `--max-worlds` budget that serves HTTP requests, at roughly 4 MiB resident each. A few thousand viewers is a `--max-worlds` and a memory decision to take before the feature ships (`docs/runbooks/sizing.md`); past the budget an upgrade is refused with `503` **before** the `101`, which a browser reports as `onerror` and `close 1006` with an empty reason — indistinguishable from a network failure, so a client that wants to back off differently needs the server to say so another way (a `GET` health route before connecting, for instance).

**Cookies in the browser.** `cookies.serialize` defaults to `Secure; HttpOnly; SameSite=Lax`. Chrome and Firefox accept a `Secure` cookie from `http://localhost` and loopback (a secure-context exception); **Safari does not**, and neither does any browser for a plain-http LAN address — so in development either serve the dev flow through the frontend's proxy on `localhost`, or make `secure` follow the environment (`cookies.serialize("sid", value, { secure: ctx.env.APP_ENV !== "development" })`; never ship it off). `SameSite=Lax` is the CSRF posture: the cookie is not sent on a cross-site POST, and a contract route only accepts `application/json` bodies (a form post is `415`), so a classic CSRF form cannot reach a handler; a cross-site `fetch` with `credentials: "include"` is blocked by the browser unless the proxy's CORS policy lists the origin. `SameSite=None` needs `secure` and an explicit reason. `EventSource` sends cookies only same-site, or cross-site with `{ withCredentials: true }` and a CORS policy that allows credentials; a browser's `WebSocket` sends them on the upgrade by itself, which is how `auth.cookie` works on a socket route.

**Browsers and credentials.** `EventSource` and `new WebSocket(url)` cannot set headers. For a WebSocket with `auth: bearer`, the browser passes the token as the second subprotocol — `new WebSocket(url, ["bearer", token])` — and the server echoes `Sec-WebSocket-Protocol: bearer`; the same works for `auth.header` schemes (the second entry is the header's value). The auth resolver runs **before the upgrade completes**: a refused credential is an ordinary `401` response, not a `101` followed by a close frame, so a client can tell "log in again" from "retry". For SSE from a browser, use `fetch` with a header and a stream reader (or a cookie-based custom scheme) — `EventSource` sends no headers. Declare the events a stream emits — `http.stream("/events", { events: { tick: z.object({ i: z.number() }), done: z.object({ total: z.number() }) } }, …)` — and `stream.event("tick", data)` validates `data` before writing it (a mismatch is `event_contract_violation`: the stream ends early, the log names the event), while the OpenAPI document lists the events on the response and types each payload as `components.schemas.<OperationId>Event<Name>`, so a client knows what to `addEventListener` for and what arrives. `stream.event(name, data, { id, retry })` writes the `id:`/`retry:` fields too: a reconnecting `EventSource` sends the last id back as the `last-event-id` header, so a handler that starts from `ctx.headers["last-event-id"]` resumes where the client left off; `retry` is the browser's reconnect delay in milliseconds. A client that leaves (a closed tab, a reconnect) cancels the world — the normal end of a stream, logged at debug, not a failed stream. A stream is not only SSE: `http.stream("/export.csv", { contentType: "text/csv" }, async (ctx, stream) => { await stream.send("id,name\\n"); … })` streams a CSV (or `application/x-ndjson`, or anything) — the declared type is the response's `content-type` unless `stream.start({ headers })` says otherwise, and it is what the OpenAPI document and the reference say the endpoint streams. A handler that throws after `start()` cannot change the 200 already sent: the body ends early, the runtime logs it (`stream handler failed after the head was sent`) and counts it (`usai_http_streams_failed_total`); for a download, end with a sentinel the client checks (a trailer row, a final `event: done`). The message contracts (`incoming`/`outgoing`) appear in the OpenAPI document (`x-usai-socket`) and on the socket's page in `/_usai/docs`; an event stream carries `Cache-Control: no-cache`.

**CORS.** The runtime has no CORS feature: it does not answer preflights or add `Access-Control-*` headers by itself, and `OPTIONS` on an application route is a `405` with an `Allow` header. In production the reverse proxy in front owns it (Caddy: `header Access-Control-Allow-Origin …` and a `@options` matcher, §14), or the frontend is served from the same origin. **In development**, with a Vite/Next dev server on `localhost:5173` and the API on `127.0.0.1:3000`, the cheapest answer is the dev server's proxy (`server.proxy: { "/api": "http://127.0.0.1:3000" }` in `vite.config.ts`): same origin for the browser, no CORS at all, cookies work. If you must talk cross-origin, the primitives are there: a declared `http.options("/products", {}, async () => http.noContent({ "access-control-allow-origin": "http://localhost:5173", "access-control-allow-methods": "GET, POST", "access-control-allow-headers": "content-type", "access-control-allow-credentials": "true" }))` answers the preflight — or one `http.options("/*any", …)` catch-all for every path (a declared route wins over the 405) — and `defineApp({ headers: { "access-control-allow-origin": …, "access-control-allow-credentials": "true", "access-control-expose-headers": "etag, location, x-request-id" } })` puts the headers on every response, the runtime's own 401/404/413 included. It is a static origin with no `Vary: Origin` and the OPTIONS route appears in the OpenAPI document — fine for dev, not what to ship.

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
  const api = ctx.resources.payments;   // typed from `resources: [payments]`
  const res = await api.fetch("/v1/charges", { method: "POST", json: ctx.body });
  if (!res.ok) throw errors.unavailable(`payments answered ${res.status}`);
  return res.json();
});
```

There is no global `fetch` in a world (calling it rejects with `fetch_not_available` and this advice). Outbound HTTP is a **declared resource**: the runtime owns the client (pool, TLS roots, redirects, timeouts) and every `fetch` is an operation owned by the world — cancelled with it, bounded by its deadline. `baseUrl`/`baseUrlEnv` pins the destination (another origin is `origin_refused`); without it the client may call any http(s) URL. `maxConcurrent` refuses (503) instead of queueing. Responses: `status`, `ok`, `headers`, `text()`, `json()`, `bytes()`; a non-2xx status is data, not an exception. The destination shows in `usai graph`, `usai inspect` and `/_usai/docs`, and its counters in `/_usai/status`.

## 12. What a world can use

A world is a bare JavaScript engine with exactly these globals, no more: `console`, `setTimeout`/`clearTimeout`/`setInterval`/`clearInterval`, `queueMicrotask`, `atob`/`btoa`, `TextEncoder`/`TextDecoder`, `URL`/`URLSearchParams`, `structuredClone`, `crypto` (below), plus the SDK's `ctx`. Put `"types": ["@sakaladev/usai/globals"]` in `tsconfig.json` (the scaffold does) instead of the `DOM` lib or `@types/node`, so the compiler knows the same set.

`crypto` is a WebCrypto subset with a clear lifetime story: `crypto.randomUUID()`, `crypto.getRandomValues(typedArray)` (32 bytes of host entropy per world, expanded with SHA-256 — never the image's state), `crypto.subtle.digest("SHA-256" | "SHA-384" | "SHA-512", data)`, and HMAC through `subtle.importKey("raw", key, { name: "HMAC", hash: "SHA-256" }, …)` + `subtle.sign`/`subtle.verify` (constant-time compare). Password hashing is `password.hash(plain)` / `password.verify(plain, hash)` from the SDK — Argon2id run by the host off the world's thread; store the returned PHC string.

Deliberately absent, and why:

- **`fetch`** — outbound HTTP is the `httpClient` resource (§11); the global only explains that.
- **Bytes and text at speed.** `TextEncoder`/`TextDecoder`, `atob`/`btoa` and the SDK's `bytes.toBase64/fromBase64` are native in the Wasm core (memcpy-speed: 3 MB decodes in ≈0.1 s); a request body of a few MB is fine to decode in one call. What is *not* native is your own loop over the bytes — the interpreter runs ≈0.3–0.5 µs per iteration (§18), so a hand-written CSV parser over 30 000 lines is seconds, and belongs either in SQL (`unnest`, `copy`-shaped inserts) or in blocks with an `await` between them.
- **The synchronous CPU slice.** One uninterrupted synchronous run of guest code is bounded at **5 s** (a `while (true) {}` cannot hold a core forever); the world faults with `runtime_fault` and the log names it (`guest exceeded the synchronous CPU slice`). Any `await` of a host operation (`ctx.sleep(0)` will do) ends the run and starts a new slice; the request's deadline (30 s by default) still bounds the whole. The slice is not configurable.
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
curl :8080/_usai/status        # runtime truth: gauges, revisions, services, tasks, resources, http (responses by class, rejections by reason, per-workload counts, a cumulative latency histogram with its bucket bounds under "le")
curl :8080/_usai/metrics       # Prometheus text
usai generate openapi --out openapi.json           # internal profile: everything the runtime knows (x-usai-*)
usai generate openapi --public --out openapi.json  # consumer contract only: what ships to API consumers
usai graph                     # workload → resource / dispatch graph
usai bench --path /users -c 16 -d 30   # engineering load test
usai run --artifact .usai/build --control 127.0.0.1:3900   # the orchestrator's surface: install/activate/drain a revision without a restart (docs/CONTROL-API.md)
```

Every flag above has an environment form (`USAI_MAX_WORLDS`, `USAI_DRAIN_GRACE`, `USAI_STATUS_ADDR`, …); `docs/ENVIRONMENT.md` lists them all, with the tuning knobs that are *not* application configuration.

Under `usai run`, Ctrl-C or SIGTERM first fails readiness and closes connections after their response for `--drain-grace` seconds (`USAI_DRAIN_GRACE`, default 2: a load balancer stops routing here before the listener closes — the zero-502 rolling restart, `docs/runbooks/deploy-and-rollback.md`; `usai dev` has no balancer and closes its listener at once), then drains in-flight work with a bound (`--drain-timeout <s>` / `USAI_DRAIN_TIMEOUT`, default 30 s; set the orchestrator's grace period above grace + drain — the compose files use 35 s); a second signal skips the grace, a third forces exit. Read `docs/THREAT-MODEL.md` before exposing anything.

What runs where: an instance started with `usai run` runs the HTTP listener, the cron scheduler, the queue consumers **and every `service()`** of the application, each on every replica unless switched off — `--no-cron` (keep exactly one), `--no-queue` (consumers share work safely; this dedicates replicas), `--no-services` (each instance that runs them has its own copy of every service). **One-shot commands never run services or consumers**: `usai app`, `usai task run`, `usai cron run`, `usai queue run`, `usai db migrate|status|seed` and the test harness's runtime activate the revision for exactly the world they were asked for, so a `db migrate` job does not start your ingest loop against production. `/_usai/status` `scheduler` and the `usai_scheduler{kind}` gauge say which of the three an instance runs.

**What the app listener exposes.** Without `--status`, the application listener serves the application and nothing under `/_usai/` — a 404 for every one of those paths, the same one an unknown route gets, whether or not a status token is configured (a 401 there would announce that there is an operator surface somewhere): status, metrics, health and the reference (`/_usai/docs`, `/_usai/openapi.json`) live on the status listener (`--status-addr`). `--status` puts them all on the app listener for development and trusted networks — then anyone who can reach the API can read per-route counters, the process's RSS and the *full* OpenAPI profile (environment variable names, cron schedules, queue message schemas). To publish the API reference to consumers, expose `/_usai/openapi.json?profile=public` through the proxy, or `usai generate openapi --public` into your docs.

Health: `GET /_usai/live` (the process answers) and `GET /_usai/ready` (an active revision exists and every bound resource answers a 1 s probe — PostgreSQL runs `SELECT 1` on a leased connection; 503 names the failing resource; an empty `resources: {}` means every probe passed — only failures are listed). Route traffic on ready, restart on live. **What that couples**: a proxy removes an unready upstream, and readiness fails when *any* bound resource fails its probe — so when every replica shares one database, a PostgreSQL outage takes every route out of rotation, including routes that never query it. That is the default because a replica that cannot reach its database cannot serve most of an application and a rollout should stop rather than go live broken; `USAI_READY_REQUIRES_RESOURCES=0` reports the failing resource without failing readiness, for deployments that would rather keep resource-free routes answering (`docs/runbooks/postgres-down.md`). Draining fails readiness either way. Between probes, `/_usai/status` → `resources[].ready` says what the *last contact* with each resource was: false from the first connection-level failure (`pool_error`, `connection_closed`, SQLSTATE 08xxx/57P0x — with `detail.lastError` and `detail.unreadyForSeconds`), true again after the next successful operation; a query's own error never flips it. While a dependency is down the runtime writes **one `WARN dependency unavailable` line per error code per second** with a `suppressed` count, not a stack per request (`docs/runbooks/postgres-down.md`). A `service()` that has exhausted its restart policy is `failed` in `/_usai/status` and `usai_service{state="failed"}` and logs `service gave up`; it does **not** make the instance unready (HTTP is still served) — alert on the gauge. `usai probe live|ready --addr <status listener>` is the same check as a command with exit code 0/1 — it reads `USAI_STATUS_ADDR` too, so one variable configures the server and the probe. It is for a Docker `HEALTHCHECK`, where the image has to make the request itself. **A Kubernetes `httpGet` probe does not need it**: the kubelet makes that request, so the image needs no HTTP client at all — use `httpGet` and keep `usai probe` for an `exec` probe you have a reason to prefer, with `timeoutSeconds` above the command's own (`--timeout`, 3 s by default) — if the supervisor's is the shorter one it kills the check instead of reading its answer.

**Reading the application from a terminal.** `/_usai/docs` is a single-page shell that fetches the document at load, so `curl`ing it returns markup and no routes; `/_usai/openapi.json` is the document itself, and `usai inspect` needs no server at all. Grep those two.

**What the proxy owns.** Three things an Express or Laravel application does in-process are the reverse proxy's here, on purpose: **CORS** (preflights and `Access-Control-*`, §9), **static files** and the **access log** (below). **Security headers** can live either side: `defineApp({ headers: { "strict-transport-security": "max-age=31536000", "x-content-type-options": "nosniff" } })` sets static headers on every application response (a handler's own header of the same name wins; `/_usai/*` is untouched), which is what a deployment without a proxy in front needs; a proxy that already sets them is the better place, because it covers the assets too. **Static files** (a world has no filesystem and the build has no asset loader — `import "./index.html"` fails; serve assets from the proxy or object storage; a small inline HTML string through `http.raw` is the exception) and the **access log** (below). One Caddy block covers all four:

```caddyfile
api.example.com {
	header {
		Strict-Transport-Security "max-age=31536000; includeSubDomains"
		X-Content-Type-Options nosniff
		Referrer-Policy strict-origin-when-cross-origin
	}
	@preflight method OPTIONS
	handle @preflight {
		header Access-Control-Allow-Origin https://app.example.com
		header Access-Control-Allow-Methods "GET, POST, PATCH, DELETE"
		header Access-Control-Allow-Headers "authorization, content-type"
		header Access-Control-Allow-Credentials true
		respond 204
	}
	header Access-Control-Allow-Origin https://app.example.com
	header Access-Control-Allow-Credentials true
	handle_path /assets/* {
		root * /srv/assets
		file_server
	}
	log {
		output file /var/log/caddy/api.log
	}
	reverse_proxy app:3000 {
		health_uri /_usai/ready
		health_interval 1s
		lb_try_duration 5s
	}
}
```

**Debugging: where your output goes.** `console.log`/`console.info` are INFO,
`console.warn` WARN, `console.error` ERROR and `console.debug` DEBUG — the
last one is below the default filter, so raise it (`RUST_LOG=app=debug`) when
you want it. `ctx.log.*` is the same thing under another name. `console.time(label)`,
`console.timeLog(label, …)` and `console.timeEnd(label)` are there too, and
log their elapsed milliseconds at INFO with the workload and the request id
— production-readable, not only development output. The labels live in the
world, like everything else in it: a timer started in one request cannot be
ended in another, and one left unended is simply forgotten when the world
ends (it is not detached work). Two places
behave differently, and both surprised a developer before they were written
down:

- **In a handler** (or a task, a cron tick, a consumer) the line is printed
  as the work runs, with `workload`, `world` and `request_id` on it — that is
  the ordinary case.
- **At module scope** the line runs while the application is being *defined*,
  not per request: `usai build` prints it once, `usai dev` prints it on every
  rebuild, and `usai run` never does, because the module was evaluated before
  the artifact existed and every world resumes that snapshot. A `console.log`
  at the top of `app.ts` that "does not appear" in a running server is this,
  not a lost line.

A one-shot command (`usai app`, `usai task run`, `usai db …`) keeps the
runtime quiet by default; `-v` shows its INFO lines too. In tests, every line
is available through `app.logs()` / `app.waitForLog()` (§15) as well as on
the terminal.

**Request ids.** Every request has one: the client's `x-request-id` when it sent a sane one (≤ 128 printable ASCII characters — a proxy's trace id), a UUID minted by the runtime otherwise. It comes back on the response as `x-request-id`, is `ctx.requestId` in HTTP, raw, stream and socket handlers, rides as `request_id` on every line the world logs (`ctx.log`/`console.*`, JSON and text), and goes out as `x-request-id` on every `httpClient` call the world makes — so the proxy's access log, the application's lines and the downstream service's logs join on one value without the application doing anything. **It follows the hand-off**: a task the request invokes or dispatches runs in a world of its own, and that world carries the same id (`ctx.requestId` in the task, `request_id` on its log lines, on its outbound calls), so "everything that happened for request X, including the mail job it queued" is one filter. A queue message carries its publisher's id too (`request_id` on the `usai_queue` row): the consumer's world runs under it, `ctx.requestId` says so, and a webhook fired three retries later still joins the request that caused it. A task started by hand (`usai task run`) or by cron has none.

**What a 5xx log line contains** — and does not. `application error` / `unexpected handler failure` lines carry `world`, `workload`, `request_id`, the error `code`, the error text as `error` **as the application wrote it** (`message` is the line's own text — `application error` — so the two never collide in the JSON line), `details` (for a response-contract failure: which field of the response did not match and the validator's message about it) and the source-mapped `stack`. The runtime never logs request bodies, headers, query strings, path parameters or the principal: what reaches the log about a request is what the application put in the error message and what it logged itself (`ctx.log`). Redaction is therefore the application's discipline (do not put a token or an email in an error message) plus the shipper's (a filter on `target == "app"` lines if your log pipeline redacts). Volume: one line per 5xx and per lifecycle violation, none per 2xx/4xx; application lines as written. A **lifecycle violation** is the one line whose message is not a fixed string: it is the explanation itself, several sentences of it, and the fact to alert or grep on is the structured field `code=detached_work` (`"code":"detached_work"` in JSON). Matching on the message finds nothing.

Per-request logging: there is no access log by design — 2xx and 4xx are counted (`usai_http_responses_total`, per-workload counts in `/_usai/status`), 5xx and lifecycle violations are logged with the workload and world (including a `504 deadline_exceeded`, which used to be a counter and nothing else), and `RUST_LOG='usai_runtime::observability=debug'` adds the per-world trace line and nothing else — `world`, `workload`, `request_id`, `termination`, `duration_ms` and **`cpu_us`**, which is the world's own CPU: `cpu_us` close to `duration_ms` means the handler was *computing*, far below it means it was *waiting* (`docs/runbooks/slow-route.md`). Plain `RUST_LOG=usai_runtime=debug` adds six more lines per request. Put request logging in the proxy in front.

Where the `/_usai/*` surfaces listen: `--status` puts status, metrics, live, ready and the docs on the **application** listener (development, trusted networks — a public proxy must then deny `/_usai/*`); `--status-addr 127.0.0.1:9090` (`USAI_STATUS_ADDR`) serves them on a **separate** listener instead, which is what production wants (scrape and probe a private port, expose nothing). Both listeners take a **status token** (`USAI_STATUS_TOKEN`; `--status-token` exists but a flag shows in `ps`): with one set, `/_usai/status`, `/_usai/metrics` and `/_usai/openapi.json` answer `401` without `Authorization: Bearer <token>` (Prometheus: `authorization: { credentials: … }` in the scrape config); `/_usai/live` and `/_usai/ready` stay open because probes carry no headers and reveal only "up" and the name of a failing resource, and the `/_usai/docs` shell stays open and asks for the token in the browser. Set it whenever the status listener is reachable by anything other than your scraper and your orchestrator — the pages show the process's memory, per-route counters, the revision identity and the full OpenAPI profile (environment names, cron schedules, queue schemas). And switch off what nobody consumes: `--surfaces-off docs,metrics` (`USAI_SURFACES_OFF`) makes those paths a `404` on both listeners; the names are `status`, `metrics`, `docs` (reference + OpenAPI), `live`, `ready`. A production instance that is only probed and scraped typically runs `--status-addr <private> --surfaces-off docs` with a token.

Several replicas: **WebSockets and streams need no sticky sessions** — a connection is served by whichever replica the proxy picked, and because fan-out goes through a resource rather than through process memory (§9), an event written on one replica reaches clients connected to any of them, with presence counted cluster-wide. That is the opposite of a Socket.IO deployment, which needs `ip_hash` and a Redis adapter; here there is nothing to configure, and nothing to get wrong. What a proxy *does* need is to allow the upgrade and not to time out a long read (`docs/deploy/compose.production.yaml` has the Caddy block). HTTP and queue consumers share the work without configuration (messages are claimed with `SKIP LOCKED`; migrations serialize on an advisory lock). **A cron schedule runs on every instance that schedules, unless it is declared `exclusive: true`** — then each tick is claimed once in PostgreSQL (`usai_cron_ticks`) and one instance runs it, whichever replica got there first (§6). Schedules without it need the scheduler on one replica: start the others with `usai run --no-cron` (`USAI_NO_CRON=1`); `--no-queue` likewise dedicates replicas. `SUPPORTED.md` has the topology table.

`usai top --addr <status listener>` is the status document **differenced**: requests per second, the average and the guest CPU per workload, rejections that never reached a workload, pool use and `waiting`, and memory as the number that kills the process (the cgroup's charge against its limit, RSS and PSS beside it) — over the last sample window rather than since boot, which is the number an incident is about. `-n <seconds>` is the window, `-c 1` prints one screen and exits (the form to paste into a channel), `--status-token` when the instance requires one. It reads `USAI_STATUS_ADDR`, so the same variable configures the server, `usai probe` and this.

Timing one request: `--server-timing` (`USAI_SERVER_TIMING=1`) puts `Server-Timing: total;dur=…, world;dur=…, cpu;dur=…` on every response the application produced — the request end to end, how long the world was alive (the rest is routing, decoding, validation and encoding), and how much of the world was the guest computing rather than waiting on a resource. It is timing and nothing else, so unlike `--diagnostics` it may stay on in production; a request rejected before a world exists carries no header at all. `usai dev` has it on. `docs/runbooks/slow-route.md` is the page that uses it.

Logs: `--log-format json` (global flag) writes one JSON object per line with `timestamp`, `level` and fields, on stderr, and leaves **stdout empty** for `usai run` (the startup banner becomes one `serving` line on the JSON stream), so a shipper that reads both streams of the process parses every line it sees; the application's `console.*`/`ctx.log.*` lines carry `target: "app"`, `workload`, `world`, `request_id` and appear at INFO. **Structured fields**: a trailing plain object is data, not text — `ctx.log.info("invoice paid", { invoiceId, cents })` gives `message: "invoice paid"` and `fields: {"invoiceId":…,"cents":…}`. `fields` is a **nested object** in the JSON line (`.fields.invoiceId` in `jq`, no second parse stage in a Loki/promtail pipeline); other arguments are stringified into the message as `console` does. Results of one-shot commands go to stdout.

Deploying with Docker: build the scaffold's `Dockerfile`, run it with
`DATABASE_URL` and the rest of the declared environment (`usai run` never
reads `.env`), `--read-only --tmpfs /tmp` works; `docker stop` sends SIGTERM,
which drains with the same bound as Ctrl-C. A production compose file (PostgreSQL, a one-shot
`migrate` service, the app with `--status-addr`, a proxy) is
`docs/deploy/compose.production.yaml`; the scaffold's `compose.yaml` is the
development path. **On Kubernetes**, `docs/deploy/k8s/` has the Deployment,
the probes, the migration Job, the PodDisruptionBudget and the scrape target,
with the three decisions the platform forces you to take written out: where
the scheduler runs when every pod has the same PodSpec, when migrations run,
and what a PDB means while the database is down.

How much host to give it: one instance of a production-shaped application (PostgreSQL pool, a task, a cron, status + metrics on, `--max-worlds 48`) idles at ≈40 MiB RSS / 35 MiB PSS and 0.00 % CPU, becomes ready in ≈0.6 s, and peaks near 100 MiB under a 16-client burst — **192 MiB and 1 vCPU** is the supported envelope (`SUPPORTED.md`): re-measured 2026-09-23 on a box charged for its own page cache, the production shape peaks at 106 MiB resident / 93 MiB charged whatever the box, and 128 MiB serves it without the kernel reclaiming once — 192 MiB is what covers the extra revision a rolling deployment holds. **64 MiB** runs a hello-only application; 48 MiB does not, whatever the old numbers said (it is charged to its ceiling, thrashes on its own text and serves one request per second without ever being killed — `docs/measurements/2026-09-23-floor-accounting.md`). Memory follows the peak number of concurrently used worlds (≈4 MiB RSS per touched slot, kept resident so the next world does not fault the image back in; `USAI_WASM_KEEP_RESIDENT=0` returns it after a burst at half the throughput), not the request count, so `--max-worlds` is the memory knob and `mem_limit` should cover `40 MiB + max_worlds × 4 MiB`. A fractional vCPU works; its burst p99 is the CFS throttle. Fifty mostly-idle instances on one host cost ≈30 MiB PSS each and no idle CPU (`docs/measurements/2026-09-20-p8e-efficiency.md`).

Signed artifacts: `usai keygen` makes an Ed25519 key; `usai build --sign usai-signing.key` (or `USAI_SIGNING_KEY`) writes `signature.json` — the SHA-256 of every file in the artifact, the native `cache/image.cwasm` included, signed. A runtime started with `usai run --artifact … --require-signature <public key>` (or `USAI_REQUIRE_SIGNATURE`, a hex key or a file with one key per line) refuses, before listening, an unsigned artifact, one signed by an untrusted key, any changed file, and any file the signature does not cover; the control surface applies the same rule to installs. Keep the private key in CI's secret store and the public keys in the deployment's environment.

`/_usai/docs` is the **Application Reference**: one page per HTTP operation, stream, WebSocket, task, cron, queue consumer, service, command, resource and schema, generated from the definition the runtime is executing (the revision identity is on the overview). Beyond parameters, bodies and responses, each page says what the unit of work *does* — the lifecycle strip: which slots are refused before a world exists, the world's lifetime and effective deadline, the resources it leases per operation (linked to the resource's page, which lists who leases it), the tasks it hands off and the topics it publishes (linked to the consumer). Search (⌘K, `/`) covers the whole application; `[` and `]` step through it; the theme follows the system unless you pick one. The request panel sends real requests to this server and reports the runtime's own time (`x-usai-server-ms`); it remembers one auth value per scheme for the browser tab, so you authenticate once. Every operation has cURL, JavaScript and Python snippets built from the contracts. `defineApp({ description })` puts a paragraph on the overview and in `info.description`; declare `dispatches(endpoint, task)`, `publishes(...)` and your `errors`/`response` statuses so the page can say so.

`/_usai/openapi.json` is the same document with the runtime facts as `x-usai-*` extensions (the internal profile). `/_usai/openapi.json?profile=public` and `usai generate openapi --public` write the **public profile**: paths, parameters, bodies, responses, security schemes and the declared error codes (folded into the response descriptions), without any `x-usai-*` extension or the workload, resource and environment inventory — the file to hand to API consumers or a hosted docs product. The SDK's own reference (what `http.get` takes, what `ctx.tasks.dispatch` promises) is `docs/sdk/`, generated from the source.

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
    await app.queue("orders").deliver({ orderId: "o1" });                      // one delivery to the consumer, no queue row
    await app.command("reconcile").run(["--dry-run"]);
  } finally {
    await app.close();
  }
});
```

**Streams and sockets.** `const s = await app.stream("/events", { query: { n: 3 } }); const first = await s.next(); s.close()` reads server-sent events one by one (`event`, `data`, `json`, `id`), `s.text()` reads a non-SSE stream whole (a CSV download), and `app.socket("/chat", { headers: { cookie }, protocols: ["bearer", token] })` opens a WebSocket the way a browser would — `ws.send(message)`, `await ws.next()`, `await ws.closed()` for the server's close frame; a refused credential rejects with `TestSocketRefused` and its `status` (401) before any frame.

**What a request did after it answered.** A dispatched task runs after the response was sent; the harness keeps the runtime's log (`--log-format json`, the last 10 000 lines) and lets a test join on the request id: `const res = await app.http.post("/auth/forgot", { body, headers: { "x-request-id": "t-1" } }); const line = await app.waitForLog({ requestId: "t-1", workload: "task:send-reset-email" }); line.fields.code` — `app.logs({ requestId, workload, level, target: "app", message })` is the same filter over what was already written, `waitForLog` waits (5 s by default) for a line that has not arrived yet. Every line the application wrote with `ctx.log`/`console.*` is there at INFO (`target: "app"`, `fields` parsed), the runtime's own at WARN and above; set `RUST_LOG` to change that. The harness has **no database handle by design**: read state through the application (a route, a `command` the test runs with `app.command(...).run()`), or with the driver of your choice in the test process — the tests are Node, the application is not.

**Tests live outside the build's tsconfig.** `usai build` runs the project's own `tsc -p tsconfig.json`, and a world has no `node:test` — so a test file inside that config's `include` fails the build with `TS2307: Cannot find module 'node:test'`. The scaffold splits them: `tsconfig.json` includes `["src", "usai.config.ts"]` with the world's globals, and `tsconfig.test.json` adds `types: ["node"]` for the test files. A project wired by hand needs the same split.

`testApp` needs the `usai` binary (`USAI_BIN` or on `PATH`) and the project's declared environment (pass `env: { DATABASE_URL }`). Run the files with `node --test` (or `usai test`, which sets `USAI_BIN` and loads `.env`); the `--conditions=usai` flag the repository's own examples use selects the SDK's TypeScript sources inside this monorepo and **breaks** an installed package — it resolves to TypeScript under `node_modules`, which Node refuses to strip (`ERR_UNSUPPORTED_NODE_MODULES_TYPE_STRIPPING`). Leave it out, or run `usai test`, which sets what it needs. **Inside this repository it is the opposite**: a test run with a bare `node --test` there resolves the SDK's *built* `dist`, which may be stale or missing a newer surface, and the symptom is a clean `tsc` followed by `TypeError: app.socket is not a function` — a method the types promise and the loaded copy does not have. `usai test` is the answer in both places; reach for the flag only when running `node --test` by hand in this workspace. With `migrate: true` (or `migrate: { seed: true }`) it runs `usai db migrate` / `usai db seed` first — for a throwaway database (`usai test` reads `.env`, so point `DATABASE_URL` at one that may be wiped). The harness starts the runtime with `--diagnostics`: every `TestResponse` carries `violations` (the world's lifecycle violations, e.g. `detached_work` for an un-awaited `dispatch`) — assert `deepEqual(res.violations, [])` on the requests that matter — and 500 bodies carry the error details. Lifecycle-specific tests are ordinary: mutate in one request, read in the next, and assert the mutation is gone.

A `WARN connection quarantined: original query has no terminal outcome` in a test run is not a failure: the harness ends worlds while statements are still in flight, and a connection whose outcome the runtime cannot prove is replaced rather than reused (C5, `docs/runbooks/postgres-down.md`). It is the contract working.

In CI, the tests need the binary and a PostgreSQL. The pure-npm path fetches the release binary on `pnpm install` (`@sakaladev/usai`'s wrapper; `USAI_RELEASE_BASE` for a mirror), and a service container is the database:

```yaml
# .github/workflows/test.yml
jobs:
  test:
    runs-on: ubuntu-latest
    services:
      postgres:
        image: postgres:18
        env: { POSTGRES_USER: app, POSTGRES_PASSWORD: app, POSTGRES_DB: test }
        ports: ["5432:5432"]
        options: --health-cmd "pg_isready -U app" --health-interval 2s --health-timeout 2s --health-retries 30
    steps:
      - uses: actions/checkout@v4
      - uses: pnpm/action-setup@v4
      - uses: actions/setup-node@v4
        with: { node-version: 24, cache: pnpm }
      - run: pnpm install --frozen-lockfile
      - run: pnpm usai test
        env:
          DATABASE_URL: postgres://app:app@127.0.0.1:5432/test
```

`pnpm usai test` runs `node --test` over the project with `USAI_BIN` set to the fetched binary; `testApp({ migrate: true })` applies the migrations to the service database before the first test. One database per job — the tests wipe what they create, not what a neighbour created.

`app.http.post(path, { body })` sends an object as JSON (`content-type: application/json`) and a string byte-for-byte (for signed webhook bodies: sign the string, send the string). Every call returns `{ status, headers, body, text }` — `body` is parsed JSON when the response is JSON.

## 16. A realistic application: `examples/todos`

*(The examples in this repository are wired to it: their `tsconfig.json` extends the workspace base and maps `@sakaladev/usai` to the SDK's sources. Read them here; to **start** a project run `pnpm dlx @sakaladev/create-usai <dir>`, whose scaffold is self-contained.)*

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

Per-world cost on the Wasm substrate is flat with respect to application size: instantiating a world from the pre-initialized image costs ~0.02 ms whatever the bundle contains, and validators declared as contracts are prepared before the image is snapshotted, so a fresh world does not rebuild them. Numbers belong with their conditions: a contract-validated hello request is 0.90 ms p50 at c=1 on the qualification VM (release build, pinned core, a soak as co-tenant — `docs/measurements/2026-09-19-p8-parity.md`), and the comparators on the same host are in the same report; quote from there, not from here. Handler code runs in an interpreter compiled by Cranelift: CPU-heavy loops are slower than on a JIT; keep hot loops small or move them to the database.

**A `usai dev` rebuild takes about three seconds** on an idle machine (half of it compiling the application's image), and a change that does not alter what the application declares is recognised as such — the running revision keeps serving. Coming from `tsx watch` that is slower; it buys a revision that is byte-for-byte what production runs.

**The first request after a start is a little slower than the rest** — on `examples/hello` (release build, idle machine) 7–11 ms against ~4 ms for the ones after it. The two definition-lifetime costs that used to sit there, compiling the routing table and the engine's first instantiation of the image, are paid at activation now, before readiness turns true; what remains is the guest's own first entry into your code. Nothing to configure, but worth knowing if you are reading a p99 right after a rolling restart.

## 19. Coming from Express, Fastify or Laravel

The same service, piece by piece. Left: what you write today. Right: where it lives in Usai, and the section that says how. "Proxy" means the reverse proxy in front (Caddy, nginx, an ingress) — §14 has one Caddy block that covers every row marked so.

| You have | In Usai | Where |
|---|---|---|
| `app.use(express.json())`, `body-parser`, Fastify schemas, `FormRequest` | The route's contracts (`body`, `params`, `query`, `headers` as Standard Schema): validated **before a world exists**, typed on `ctx`, in the OpenAPI document | §4 |
| Middleware chain / `$middleware` for auth | `auth.bearer` / `auth.header` / `auth.cookie` / `auth.custom`, attached per route as `auth:`; the resolver runs in the world before the handler; `ctx.auth` is the principal | §4 |
| `cookie-session`, `express-session`, Laravel sessions | `auth.cookie({ cookie: "sid" })` + `cookies.sign/verify` for a signed value, or a sessions table; `Set-Cookie` through the headers argument of `http.response` | §4 |
| `cors()`, `HandleCors` | The proxy (the runtime answers no preflight) | §9, §14 |
| `helmet()`, security headers | `defineApp({ headers })` for static headers on every response, or the proxy | §14 |
| `morgan`, access logs | The proxy's access log; the runtime counts (`/_usai/status`, `usai_http_*`) and logs 5xx and lifecycle violations with `request_id` | §14 |
| `req.id`, `X-Request-Id` middleware | Built in: `x-request-id` accepted or minted, `ctx.requestId`, on every log line and outbound call | §14 |
| `multer`, `UploadFile` | `http.raw` + `multipart.parse(bytes, contentType)`; bytes into `bytea` as a `Uint8Array`; `USAI_MAX_BODY_BYTES`; large files → presigned upload to object storage | §4 |
| `routes/` + `controllers/` + `services/` + `repositories/`, a DI container | The same layering, with one rule: a repository takes its `SqlExecutor` as an argument instead of closing over a pool — the handle lives in the world, not in module scope | §3 |
| `express.static`, `public/` | The proxy or object storage (a world has no filesystem) | §14 |
| `res.write` / SSE / `StreamingResponse` | `http.stream(path, { contentType })` — SSE by default, CSV/NDJSON declared | §9 |
| `ws`, Socket.IO, Reverb | `socket(path, { incoming, outgoing })`, one world per connection, browser credential as `["bearer", token]` | §9 |
| `setImmediate(sendEmail)`, `void promise`, `dispatch()->afterResponse()` | `task()` + `ctx.tasks.dispatch()` (owned hand-off, not durable) or the queue (durable); a forgotten `await` on a write is a `500 detached_work`, not a silent loss | §5, §8 |
| BullMQ, Sidekiq, Horizon, Celery | `queue.consume(topic, { concurrency, retry })` on PostgreSQL (`SKIP LOCKED`, explicit retry, dead letters, at-least-once); `ctx.queue.publish` | §8 |
| `node-cron`, Celery beat, `schedule()` with `onOneServer()` | `cron(name, { schedule, exclusive: true })` — one instance per tick across replicas, claimed in the database | §6 |
| Artisan commands, `manage.py` | `command(name, handler)` → `usai app <name> -- args`; `seeder()` → `usai db seed` | §6, §7 |
| Knex/Prisma migrations, Alembic | `.sql` files applied in order by `usai db migrate`, checksum-locked, in the artifact | §7 |
| Prisma, Eloquent, SQLAlchemy | Plain SQL through `ctx.resources.db` (`query/one/execute/transaction`), typed by the schema you already declared; no ORM | §7 |
| `pg.Pool` per process, `DB::connection()` | `postgres("db", { pool })` declared once, leased per operation, connections never reused without terminal proof | §7 |
| `axios`/`fetch` to other services | `httpClient("name", { baseUrlEnv })` declared as a resource; `fetch` is not a global | §11 |
| `dotenv`, `.env` in production | `env({ … })` declared; `usai dev` reads `.env`, `usai run` reads only the process environment and refuses to start without what is declared | §13 |
| `process.on("SIGTERM", () => server.close())` | Built in: readiness fails, connections close after their response, in-flight work drains within a bound | §14 |
| PM2 cluster, `pm.max_children`, Gunicorn workers | One process, many worlds in parallel (`--max-worlds`); replicas for redundancy, not for cores | `runbooks/sizing.md` |
| Supertest, `TestClient`, Pest HTTP tests | `usai/test`: `testApp()` runs the real binary; `app.http.*`, `app.task().invoke`, `app.queue().deliver`, `res.violations` | §15 |

**What your editor will suggest that does not exist here.** `req`/`res`/`next` are not the handler's
arguments (one `ctx`, and you *return* a value); `process`, `require`, `Buffer`, `__dirname` and `fs` are
declared in `@sakaladev/usai/globals` only so the compiler can say what to use instead — point `types` at it
rather than `@types/node`, which would advertise APIs a world does not have and let `process.env.X` compile
and be `undefined` at runtime. `performance.now()` exists and is monotonic from the world's start.

What has no counterpart, on purpose: module-level caches and singletons (a world starts fresh; `cache.local` is the process-wide, non-durable cache), global mutable state between requests, `fs`/`process`, an ORM, and an in-process cron that is silently duplicated by the second replica.
