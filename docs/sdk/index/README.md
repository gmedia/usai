[@sakaladev/usai](../README.md) / index

# index

`@sakaladev/usai` — declare work and resources; the runtime gives each
its natural lifetime. Public surface for v0 (breaking changes allowed
before alpha). Start with [defineApp](functions/defineApp.md), then [http](variables/http.md),
[task](functions/task.md), [cron](functions/cron.md), [queue](variables/queue.md), [postgres](functions/postgres.md); every
handler receives a [BaseContext](interfaces/BaseContext.md). What *your* application
declares — every operation, what it leases and hands off, a request
panel — is the application reference at `/_usai/docs` on a running
`usai dev`.

Vocabulary used throughout:
- **world** — the fresh, isolated execution of one unit of work (a
  request, a task, a cron tick, a queue message, a connection, a
  service). Nothing survives it except what it wrote to a resource.
- **commit** — the moment a world's work counts: its handler returned
  (for HTTP, the response head is sent; for a queue message, it is
  acknowledged). Hand-offs start after it; a throw means no commit.
- **lease** — a resource operation owned by the world for its duration
  (one connection per statement, one request per `fetch`); returned
  only after a terminal outcome.
- **hand-off** — `ctx.tasks.dispatch` or `ctx.queue.publish`: work that
  outlives the world, with a new owner, declared explicitly.
- **revision** — one immutable application definition installed in the
  runtime; a deploy installs a new one and **drains** the old (in-flight
  work finishes, persistent workloads are asked to stop).

## Application

| Name | Description |
| ------ | ------ |
| [Method](type-aliases/Method.md) | HTTP methods an endpoint can declare. |
| [DeclaredError](interfaces/DeclaredError.md) | An error a workload declares it may answer with (`errors: [...]`), so the reference and the OpenAPI document list it. |
| [AuthDeclaration](interfaces/AuthDeclaration.md) | What `auth.bearer`/`auth.header`/`auth.custom` return: a named boundary reused by reference. `Principal` is the type of `ctx.auth`. |
| [ResourceDeclaration](interfaces/ResourceDeclaration.md) | What `postgres(...)`, `cache.local(...)` and `httpClient(...)` return. A plain object: the build reads it into the manifest, the runtime owns the resource it names, and a workload lists it under `resources`. |
| [ResourcesOf](type-aliases/ResourcesOf.md) | `ctx.resources` for a workload that declared `resources: R`: one property per declaration, named by the resource, typed as its in-world handle ([PostgresHandle](interfaces/PostgresHandle.md), [CacheLocalHandle](interfaces/CacheLocalHandle.md), [HttpClientHandle](interfaces/HttpClientHandle.md)). Without a declaration list it is `Record<string, unknown>`. |
| [WorkloadPolicies](interfaces/WorkloadPolicies.md) | Bounds every workload can declare. |
| [HttpContracts](interfaces/HttpContracts.md) | The schema slots of an HTTP endpoint. Any Standard Schema (`zod`, `valibot`, `arktype`, …) works; slots whose schema can describe itself as JSON Schema are validated before a world exists, the others inside it. |
| [ResponseHeaderDocs](type-aliases/ResponseHeaderDocs.md) | Response headers an endpoint sets, documented per status for the reference and the OpenAPI document (`responses[status].headers`): the key is the status (`201`, `200`, or `"*"` for every status), the value maps a header name to one line about it. Descriptive — the runtime does not validate them; a generated client learns they exist. |
| [HttpOptions](interfaces/HttpOptions.md) | The schema slots of an HTTP endpoint. Any Standard Schema (`zod`, `valibot`, `arktype`, …) works; slots whose schema can describe itself as JSON Schema are validated before a world exists, the others inside it. |
| [ModuleDeclaration](interfaces/ModuleDeclaration.md) | What [defineModule](functions/defineModule.md) returns. |
| [AppDeclaration](interfaces/AppDeclaration.md) | What [defineApp](functions/defineApp.md) returns: the application's default export. |
| [DefineModuleOptions](interfaces/DefineModuleOptions.md) | Options for [defineModule](functions/defineModule.md). |
| [defineModule](functions/defineModule.md) | Group workloads, resources, migrations and seeders under a name. A module is organisation, not isolation: its workloads run like any other, and a resource it declares is shared with every module that declares the same one. Modules are the unit that owns SQL migrations. |
| [DefineAppOptions](interfaces/DefineAppOptions.md) | Options for [defineApp](functions/defineApp.md). |
| [defineApp](functions/defineApp.md) | The application root: the default export of the entry file. Everything the runtime will ever run is reachable from here — modules, top-level workloads, resources and the environment contract — which is why `usai inspect`, `usai graph`, the OpenAPI document and the reference page all read this one object. Declaring a workload twice, or leaving a hole in a list (a `const` used before it ran), is a build error that names the slot. |

## HTTP

| Name | Description |
| ------ | ------ |
| [RawResponse](interfaces/RawResponse.md) | Raw response for the escape hatch: bytes or text, no contract. |
| [HttpHandlerResult](type-aliases/HttpHandlerResult.md) | What an HTTP handler may return: the body (encoded as JSON with status 200, or the single declared `response` status), an explicit [HttpResponse](interfaces/HttpResponse.md) from `http.response`/`http.created`/…, a bodiless one (`http.noContent()`, `http.notModified({ etag })`) whatever the declared contract, or a [RawResponse](interfaces/RawResponse.md). Promises of any of these are awaited. |
| [HttpContext](interfaces/HttpContext.md) | The context of one HTTP request, typed from the declared contracts: `params`, `query`, `headers` and `body` carry the schemas' output types (already validated at the boundary, before this world existed), `auth` carries the principal the auth declaration resolved. Everything on [BaseContext](interfaces/BaseContext.md) is available too. The world lives for this request only; nothing here survives the response. |
| [RawContext](interfaces/RawContext.md) | The context of a raw request (`http.raw`): no contracts, the exact bytes on `request`. Read the body once. |
| [RawOptions](interfaces/RawOptions.md) | Options for `http.raw`. |
| [http](variables/http.md) | Declare HTTP endpoints. Each `http.<method>(path, options, handler)` returns a [Workload](interfaces/Workload.md) to list in `defineApp`/`defineModule`. |
| [MultipartFile](interfaces/MultipartFile.md) | One file part of a multipart body. |
| [MultipartBody](interfaces/MultipartBody.md) | A parsed `multipart/form-data` body. |
| [parseMultipart](functions/parseMultipart.md) | Parses a `multipart/form-data` body. `contentType` is the request's `content-type` header (the boundary is read from it). Throws a `TypeError` on a malformed body — answer 400 with it. |
| [multipart](variables/multipart.md) | The multipart helper as one object. |
| [MultipartPart](type-aliases/MultipartPart.md) | One part to encode: a text field, or a file with its bytes. |
| [encodeMultipart](functions/encodeMultipart.md) | Encodes a `multipart/form-data` body — what a browser form or `curl -F` sends — for a test or an outbound call: `const { body, contentType } = multipart.encode([{ name: "file", filename: "a.csv", data }])`, then `app.http.post("/imports", { body, headers: { "content-type": contentType } })`. |

## Streams and WebSockets

| Name | Description |
| ------ | ------ |
| [StreamHandle](interfaces/StreamHandle.md) | The second argument of an `http.stream` handler: the response, chunk by chunk. The first `send`/`event`/`start` **commits** the status and headers; after that the world is bound to the connection and ends when the handler returns, the client disconnects (the world is cancelled), or the revision drains. |
| [StreamContext](interfaces/StreamContext.md) | The context of a streaming request: request facts plus [BaseContext](interfaces/BaseContext.md). |
| [StreamOptions](interfaces/StreamOptions.md) | Options for `http.stream`. |
| [SocketContext](interfaces/SocketContext.md) | The context of a WebSocket connection, shared by `open`, `message` and `close`: request facts, `send`/`close`, connection-local `state`, and in `message` the validated incoming `message`. |
| [SocketOptions](interfaces/SocketOptions.md) | Options for [socket](functions/socket.md). |
| [SocketHandlers](interfaces/SocketHandlers.md) | The three moments of a connection. |
| [socket](functions/socket.md) | Declare a WebSocket endpoint: one **connection-bound** world per connection, from the upgrade to the close. `ctx.state` is the connection's mutable memory and ends with it; nothing is shared between connections except through resources. A client disconnect cancels the world; a draining revision closes the socket with 1012 (service restart) and `close` runs. `concurrency` bounds open connections. |

## Tasks, cron, commands, services

| Name | Description |
| ------ | ------ |
| [TaskOptions](interfaces/TaskOptions.md) | Options for [task](functions/task.md). |
| [TaskContext](interfaces/TaskContext.md) | The context of one task invocation: the validated `input` plus [BaseContext](interfaces/BaseContext.md). |
| [task](functions/task.md) | Declare a task: a named unit of finite work that other workloads invoke or dispatch, and that `usai task run <name>` runs by hand. |
| [CronOptions](interfaces/CronOptions.md) | Options for [cron](functions/cron.md). |
| [CronContext](interfaces/CronContext.md) | The context of one cron tick: `scheduledAt` (ISO 8601, the tick's nominal time) plus [BaseContext](interfaces/BaseContext.md). |
| [cron](functions/cron.md) | Declare a scheduled job. Each due tick runs in a **fresh world**, finite, bounded by `timeout`. The scheduler belongs to the revision: it starts when the revision activates and stops when it drains, so two revisions never tick the same job at once. A missed tick (the process was down) is not replayed. `usai cron run <name>` runs one tick without the clock, and `app.cron(name).run()` does the same in tests. |
| [CommandContext](interfaces/CommandContext.md) | The context of one command run: the command-line `args` plus [BaseContext](interfaces/BaseContext.md). |
| [CommandOptions](interfaces/CommandOptions.md) | Options for [command](functions/command.md). |
| [command](functions/command.md) | Declare a command: finite work run on demand from the command line (`usai app <name> [args]`), in a fresh world with the declared resources. The return value is printed as JSON; a thrown error exits non-zero. Commands are for operators (a stats report, a one-off repair), not for startup: nothing runs a command unless someone asks. |
| [ServiceContext](interfaces/ServiceContext.md) | The context of a service: [BaseContext](interfaces/BaseContext.md) plus `sleep`. Watch `ctx.signal` — it aborts when the revision drains, and `sleep` resolves early then. |
| [ServiceOptions](interfaces/ServiceOptions.md) | Options for [service](functions/service.md). |
| [service](functions/service.md) | Declare a service: the one **persistent** lifetime. One world starts when the revision activates, runs the handler, and is asked to stop (`ctx.signal` aborts) when the revision drains; a handler that ignores the signal is cancelled at the drain bound. The handler returning or throwing ends the service; `restart` decides what happens next. A service is supervised per revision, so a replacement revision gets its own instance and the old one stops with its revision. |
| [dispatches](functions/dispatches.md) | Record that `from` invokes or dispatches the tasks `to`, so that `usai graph`, `inspect` and the reference page show the edge. The annotation is for the graph only: a dispatch to a task that is not listed here still runs, and a dispatch to a task that does not exist is refused at runtime (`unknown_task`) whether or not it is listed. Returns `from` with its own type — a task wrapped here is still a [TypedWorkload](interfaces/TypedWorkload.md), so `ctx.tasks.invoke` still resolves its handler's output — so it wraps a declaration in place. See [QueueHandle.publish](interfaces/QueueHandle.md#publish) and [publishes](functions/publishes.md) for the durable, cross-process equivalent. |
| [publishes](functions/publishes.md) | Record that `from` publishes to queue topics (names, or the consuming `queue.consume` workloads) with [QueueHandle.publish](interfaces/QueueHandle.md#publish), for `usai graph` and the reference page (the consumer's page lists its publishers). Annotation only; the publish itself is `ctx.queue.publish`. Returns `from` with its own type, so it wraps a declaration in place without erasing what a task's handler returns. |
| [SeederContext](interfaces/SeederContext.md) | The context a seeder runs with: [BaseContext](interfaces/BaseContext.md). |
| [SeederDeclaration](interfaces/SeederDeclaration.md) | A seeder file's default export. Discovered by `usai db seed`, run as finite work with access to the declared resources; never part of startup. |
| [seeder](functions/seeder.md) | Declare a seeder (the default export of a file matched by the module's `seeders` globs). `usai db seed [name]` runs it as finite work with the declared resources. |

## Queues

| Name | Description |
| ------ | ------ |
| [RetryOptions](interfaces/RetryOptions.md) | Retry policy of a queue consumer. |
| [ConsumeOptions](interfaces/ConsumeOptions.md) | Options for `queue.consume`. |
| [QueueContext](interfaces/QueueContext.md) | The context of one message delivery: the validated `message`, the `attempt` number and the message `id`, plus [BaseContext](interfaces/BaseContext.md). |
| [queue](variables/queue.md) | Queue workloads: `queue.consume(topic, options, handler)`. |
| [QueueHandle](interfaces/QueueHandle.md) | `ctx.queue` in every world. |

## Resources

| Name | Description |
| ------ | ------ |
| [CacheLocalOptions](interfaces/CacheLocalOptions.md) | Options for `cache.local`. |
| [CacheLocalDeclaration](interfaces/CacheLocalDeclaration.md) | Shared across worlds, local to the runtime, not durable, may disappear on restart. |
| [cache](variables/cache.md) | Cache resources. |
| [CacheLocalHandle](interfaces/CacheLocalHandle.md) | The in-world handle for a `cache.local` resource (`ctx.resources.<name>`). |
| [PostgresOptions](interfaces/PostgresOptions.md) | Options for [postgres](functions/postgres.md). The URL itself is never in the declaration: it comes from the environment at activation. |
| [PostgresDeclaration](interfaces/PostgresDeclaration.md) | A PostgreSQL pool owned by the runtime. Each operation leases one connection; reuse follows terminal proof (contract C5). |
| [postgres](functions/postgres.md) | Declare a PostgreSQL resource. The runtime owns the pool for its whole lifetime; a workload that lists the resource gets a [PostgresHandle](interfaces/PostgresHandle.md) at `ctx.resources.<name>`, and every statement leases one connection for exactly that operation. The connection returns to the pool only after a **terminal outcome** (the result or the error arrived); a world that dies mid-statement proves nothing about the connection, so it is quarantined, then removed and replaced, never reused. Connection loss and pool exhaustion surface to HTTP callers as 503, not 500. |
| [SqlParam](type-aliases/SqlParam.md) | A statement parameter (`$1`, `$2`, …): scalars bind to their SQL type, objects and arrays bind as JSON (`jsonb`); cast in SQL when a column needs something else (`$1::uuid[]`). |
| [SqlExecutor](interfaces/SqlExecutor.md) | The statements available on a connection. Rows are plain objects keyed by column name; values arrive as JSON (uuid, timestamptz and numeric as strings, integers and floats as numbers, json/jsonb as values, `bytea` as a base64 string — `bytes.fromBase64` turns it back into a `Uint8Array`). A `Uint8Array` parameter binds to a `bytea` column. |
| [PostgresHandle](interfaces/PostgresHandle.md) | The in-world handle for a `postgres` resource. Rows are plain objects keyed by column name; values are JSON (uuid/timestamps as strings). Each statement leases its own connection; `transaction` pins one for the callback and commits when it returns, rolls back when it throws. A world that ends with the transaction still open is a lifecycle error, and the runtime rolls back on its behalf. |
| [HttpClientOptions](interfaces/HttpClientOptions.md) | Options for [httpClient](functions/httpClient.md). Secrets never go in the declaration: name the environment variables that hold them. |
| [HttpClientDeclaration](interfaces/HttpClientDeclaration.md) | Outbound HTTP, declared: the runtime owns the client (pool, TLS roots, timeouts), every request is an operation owned by the world, and the destination is visible in `usai graph` and the API docs. There is no global `fetch` inside a world. |
| [httpClient](functions/httpClient.md) | Declare an outbound HTTP client. There is no global `fetch` in a world; this is how an application calls another service, and the destination is part of the application's declared shape. Each `fetch` is one leased operation: cancelled with the world, bounded by the smaller of the world's deadline and `timeoutMs`, counted against `maxConcurrent` (the next request is refused, not queued). With `baseUrl`/`baseUrlEnv` the origin is pinned and any other origin is `origin_refused` before the request leaves. A non-2xx status is data (`ok: false`), not an exception; connection failures and timeouts throw and map to 503 for HTTP callers. |
| [FetchInit](interfaces/FetchInit.md) | Options for [HttpClientHandle.fetch](interfaces/HttpClientHandle.md#fetch). |
| [FetchResponse](interfaces/FetchResponse.md) | A completed response: the body has already arrived, so the accessors are synchronous. |
| [HttpClientHandle](interfaces/HttpClientHandle.md) | The in-world handle for an `http.client` resource (`ctx.resources.<name>`). |

## Authentication

| Name | Description |
| ------ | ------ |
| [ResolverContext](type-aliases/ResolverContext.md) | What a resolver runs with: the workload's context plus the request's envelope; `resources` typed from the scheme's own `resources: [...]`. |
| [AuthRequest](interfaces/AuthRequest.md) | What an auth resolver sees of the request: method, path, headers and query — never the body. The resolver runs **inside the request's world**, after the boundary validated the request and before the handler (ADR-0004): a session lookup is an ordinary query on the scheme's own `resources: [db]` (typed on `ctx.resources`; every workload that uses the scheme leases them too), and `ctx.env` is the application's. |
| [BearerOptions](interfaces/BearerOptions.md) | Options for `auth.bearer`. |
| [HeaderOptions](interfaces/HeaderOptions.md) | Options for `auth.header`. |
| [CookieOptions](interfaces/CookieOptions.md) | Options for `auth.cookie`. |
| [CustomOptions](interfaces/CustomOptions.md) | Options for `auth.custom`. |
| [auth](variables/auth.md) | Declare an authentication boundary. Attach it to a workload with `auth: <declaration>`; `resolve` runs before the handler, with the workload's `ctx` — its declared resources plus the scheme's own `resources`, typed — and `ctx.request`, and the principal it returns is `ctx.auth`, typed. A missing credential or a thrown `errors.unauthorized()` answers 401 and the handler never runs. Authentication (who) lives here; authorization (may they) is business logic in the handler. One declaration is reused by reference across endpoints; its `name` is the OpenAPI security scheme. In v0 the resolver is application code and runs inside the request's world (ADR-0004); the rest of the boundary — routing, decoding, schema validation — runs before any world exists. |
| [CookieAttributes](interfaces/CookieAttributes.md) | Attributes of a `Set-Cookie` value. Secure, HttpOnly and `SameSite=Lax` are the defaults: a session cookie that a script can read or that travels over http is the exception, and has to be asked for. |
| [parseCookies](functions/parseCookies.md) | Parses a `cookie` request header into a name → value map (the first occurrence of a name wins, as browsers order them by specificity). Values are percent-decoded when they decode; a malformed pair is skipped. |
| [serializeCookie](functions/serializeCookie.md) | Serialises one `Set-Cookie` header value. The value is percent-encoded where the cookie grammar requires it, so anything round-trips through `parseCookies`. |
| [signCookieValue](functions/signCookieValue.md) | `value.signature` — a value the client can read but not alter. The signature is HMAC-SHA256 over the value with `secret`, base64url. Rotate by verifying against several secrets and signing with the newest. |
| [verifyCookieValue](functions/verifyCookieValue.md) | The value behind a `signCookieValue` result, or `null` when the signature does not match any of the secrets (constant-time compare per secret). |
| [cookies](variables/cookies.md) | The cookie helpers as one object, for `import { cookies } from "@sakaladev/usai"`. |
| [CredentialLocation](interfaces/CredentialLocation.md) | Where a custom scheme's credential travels: `{ in: "cookie", name: "sid" }`, `{ in: "header", name: "x-session" }`, `{ in: "query", name: "token" }`. |
| [TokenOptions](interfaces/TokenOptions.md) | Options for `tokens.sign`. |
| [TokenClaims](type-aliases/TokenClaims.md) | What `tokens.verify` returns: the payload as signed, plus the claims `sign` added. |
| [signToken](functions/signToken.md) | Signs `payload` (a JSON object of your claims — a user id, a role) with `secret`, adding `iat` and `exp`. The result is `<payload>.<signature>`, both base64url, opaque to the client, verifiable by any instance that holds the secret. |
| [verifyToken](functions/verifyToken.md) | The claims of a token `signToken` produced, or `null` when the token is malformed, expired, or signed with none of the secrets (pass an array to rotate: verify against old and new, sign with the new). Comparison is constant-time per secret. Nothing about *why* it failed is returned — a client gets `401 unauthorized` either way, and the reason is not its business. |
| [tokens](variables/tokens.md) | The token helpers as one object, for `import { tokens } from "@sakaladev/usai"`. |

## Errors

| Name | Description |
| ------ | ------ |
| [UsaiErrorShape](interfaces/UsaiErrorShape.md) | The wire shape of an application error: `{ "error": { code, message, details? } }`. |
| [UsaiError](classes/UsaiError.md) | An error with a `code` and an HTTP `status`. Thrown from a handler it becomes the response `{ "error": { code, message, details? } }` with that status; from a task or queue message it is the outcome's error. Any other thrown value is a 500 `internal` with a sanitized message. |
| [isUsaiError](functions/isUsaiError.md) | Whether a caught value is a [UsaiError](classes/UsaiError.md) (also across module copies). |
| [errors](variables/errors.md) | Constructors for the common [UsaiError](classes/UsaiError.md)s. Each takes an optional message (default: the code, spaced) and `details` (any JSON, echoed to the client — keep it safe to show). List the codes a workload throws in its `errors` option so the reference and the OpenAPI document say so. |

## Environment

| Name | Description |
| ------ | ------ |
| [EnvKind](type-aliases/EnvKind.md) | The kinds an environment field can have; `secret` is never printed by `inspect`. |
| [EnvField](interfaces/EnvField.md) | One declared variable: kind, whether it is required, and its parser. |
| [EnvDeclaration](interfaces/EnvDeclaration.md) | The application's environment contract (`defineApp({ env })`). |
| [EnvValues](type-aliases/EnvValues.md) | The typed values of a declaration: `EnvValues<typeof spec>`, where `spec` is what `env({...})` returned (a bare field map works too). |
| [env](functions/env.md) | Declare what the application needs from its environment. Values are read by the host when a revision **activates** — a missing required variable or an unparsable value fails activation, never the first request — and reach handlers as `ctx.env`, parsed. Variables a resource names (`DATABASE_URL`, `baseUrlEnv`) are required by that resource and need no declaration here. `usai run` never reads `.env`; `usai dev` does. |
| [resolveEnv](functions/resolveEnv.md) | Resolve declared values from a raw map (what the host does at activation). Throws on the first violation, naming the variable. |

## Passwords

| Variable | Description |
| ------ | ------ |
| [password](variables/password.md) | Password hashing as a host operation. Argon2id is meant to be expensive, so it runs on the runtime's blocking pool, never on a world's thread; each call is one owned operation, cancelled with the world. The hash is a PHC string (`$argon2id$v=19$m=19456,t=2,p=1$…`) to store as text. |

## Context

| Interface | Description |
| ------ | ------ |
| [ConsoleLike](interfaces/ConsoleLike.md) | What `ctx.log` and `console` offer inside a world. |
| [UsaiAbortSignal](interfaces/UsaiAbortSignal.md) | `ctx.signal`: aborts when this world is asked to stop or is cancelled. A revision drain *asks* first — services, connections, dispatched tasks, cron ticks and queue messages see `aborted` become true and a pending `ctx.sleep` return, while operations keep running, so a loop can record where it got to and return; what has not returned by the drain bound is cancelled outright (no more code runs). A client that went away, a deadline, or a cancelled `invoke` owner cancel outright too: pending host operations reject with `cancelled` and the world ends. |
| [TaskHandle](interfaces/TaskHandle.md) | `ctx.tasks`: the two ways to start a task, and the whole difference between them is who owns the child world. |
| [BaseContext](interfaces/BaseContext.md) | What every handler receives, whatever the workload kind. Everything asynchronous here is a host operation **owned by this world**: it is cancelled when the world is, and a finite world may not end while one is still pending (that is a lifecycle error with a diagnostic, not a leak). Nothing on the context survives the world. |

## Schemas

| Name | Description |
| ------ | ------ |
| [StandardSchemaV1](interfaces/StandardSchemaV1.md) | The Standard Schema v1 interface, inlined (no dependency). |
| [AnySchema](type-aliases/AnySchema.md) | Any Standard Schema (`zod`, `valibot`, `arktype`, …): what every contract slot accepts. |
| [Output](type-aliases/Output.md) | The output type of a schema, as handlers see it. |

## Build

| Name | Description |
| ------ | ------ |
| [MANIFEST\_VERSION](variables/MANIFEST_VERSION.md) | The manifest format this SDK writes; the runtime states which formats it understands and refuses the others with a rebuild hint. |
| [GUEST\_ABI](variables/GUEST_ABI.md) | The host↔guest contract this SDK's in-world runtime speaks (`docs/GUEST-ABI.md`). Stamped into `builtWith.abi`; a runtime with a different bridge refuses the artifact at install instead of faulting every world. |
| [Manifest](interfaces/Manifest.md) | What `usai build` writes to `manifest.json`: the application as data — every workload with its trigger, contracts (JSON Schema) and policies, every resource with its secret-free configuration, the auth schemes, the environment contract. Mirrors the runtime's `Manifest` exactly. |
| [describe](functions/describe.md) | Turn an [AppDeclaration](interfaces/AppDeclaration.md) into its [Manifest](interfaces/Manifest.md). The build phase calls it inside a capability-less world; call it yourself to assert on an application's shape in a unit test. Throws on a duplicate workload, a conflicting resource redeclaration, or a hole in a list. |

## Declarations

| Name | Description |
| ------ | ------ |
| [TypedWorkload](interfaces/TypedWorkload.md) | A [Workload](interfaces/Workload.md) that remembers what its handler returns, so `ctx.tasks.invoke(thatTask)` is typed instead of `unknown`. The parameter is a phantom: nothing carries it at runtime, and a `TypedWorkload` is a `Workload` everywhere one is expected. |
| [TaskOutput](type-aliases/TaskOutput.md) | What `ctx.tasks.invoke` resolves to for a given task declaration. |

## Other

| Name | Description |
| ------ | ------ |
| [bytes](variables/bytes.md) | - |
| [Workload](interfaces/Workload.md) | - |
| [HttpResponse](interfaces/HttpResponse.md) | - |
| [RawRequestBody](interfaces/RawRequestBody.md) | The exact bytes of a raw request, decoded on demand: `await ctx.request.bytes()`, `await ctx.request.text()`, or `await ctx.request.json()`. Each is a method (the body is not read until asked for). |
| [Declare](type-aliases/Declare.md) | The signature of `http.get`/`post`/…. |
| [RawHandler](type-aliases/RawHandler.md) | The handler of `http.raw`. |
| [StandardSchemaV1](namespaces/StandardSchemaV1/README.md) | - |
