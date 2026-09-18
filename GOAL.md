# Usai — Product & Development Goal

> **A program should live only as long as its work requires.**

## 1. Purpose

This document is the product and engineering north star for the production Usai repository.

It defines:

* what Usai is trying to become;
* the developer programming model;
* the runtime and lifecycle model;
* the application/project model;
* the role of configuration;
* the expected developer experience;
* the initial workload and resource primitives;
* the tooling surface;
* the implementation sequence;
* the boundary between ordinary engineering and formal research.

This is not a sealed research artifact and it is not a frozen API specification.

It is a living product direction.

The historical research repository remains the source of evidence for why the runtime is architected around explicit lifetimes, disposable execution worlds, persistent runtime-owned resources, ownership-aware cancellation, pooling/COW, and bounded host-side bookkeeping.

The production repository should inherit the **contracts and lessons**, not the experiment-oriented code structure.

---

# 2. Product Thesis

Usai is a **lifecycle-native application runtime**.

Its central idea is not merely "fresh context per HTTP request".

Its central idea is:

> **Different kinds of work have different natural lifetimes, and the runtime should model those lifetimes explicitly.**

A persistent runtime does not imply persistent mutable application state.

A request, cron invocation, queue message, task, WebSocket connection, stream, and long-running service are all application work, but they do not naturally live for the same amount of time.

Usai should let developers declare **what kind of work exists**, while the runtime gives that work the appropriate execution lifetime.

The product should make the following principles concrete:

> **Persistent runtime does not imply persistent application state.**

> **Ephemeral by default; persistent by intent.**

> **Nothing should share a lifetime merely because it shares a process.**

> **Death is not cleanup. Ownership is cleanup.**

> **Work should live only as long as the work requires.**

---

# 3. What Usai Is

Usai is intended to become a standalone runtime for backend applications that combines:

* a persistent host runtime;
* immutable application definitions;
* lifecycle-aware workloads;
* disposable execution worlds where appropriate;
* explicit persistent resources and capabilities;
* ownership-aware asynchronous execution;
* contract-aware boundaries for input and output;
* application introspection and generated metadata;
* developer tooling built from the same application model the runtime executes.

Usai should be independently useful.

It may integrate deeply with Sakala in the future, but local development, build, execution, and core runtime behavior must not require Sakala.

Sakala is a future first-class ecosystem integration, not a runtime dependency.

---

# 4. What Usai Is Not

Usai is not initially:

* a new programming language;
* a Node.js compatibility project;
* a clone of Encore;
* a clone of Cloudflare Workers;
* a clone of PHP-FPM;
* a general distributed scheduler;
* an ORM;
* a deployment platform;
* a package registry;
* a plugin marketplace;
* a microVM platform;
* a custom JavaScript engine project;
* a framework that dictates MVC, DDD, hexagonal architecture, repositories, controllers, or services.

Usai should be opinionated about **execution, lifecycle, contracts, ownership, resources, and application topology**.

It should remain largely unopinionated about **domain architecture and business-code organization**.

A useful rule:

> **Usai owns application topology. Developers own code organization.**

---

# 5. Developer Mental Model

A developer using Usai should think primarily about two questions:

1. **What work am I defining?**
2. **What must outlive that work?**

They should not need to think about:

* Wasmtime stores;
* engine instances;
* pooling allocator internals;
* COW implementation;
* pagemap scanning;
* VM slots;
* runtime reset details;
* capability bookkeeping;
* research-era R1/R2/R3 terminology.

Those are runtime implementation concerns.

The application model should look conceptually like:

```text
Application
│
├── Workloads
│   ├── HTTP request
│   ├── task
│   ├── cron invocation
│   ├── queue message
│   ├── command
│   ├── WebSocket connection
│   ├── stream
│   └── service
│
└── Resources
    ├── PostgreSQL
    ├── cache
    ├── secrets/config
    ├── storage
    ├── queue connections
    └── future capabilities
```

The key rule is:

```text
workload kind
    ↓
default execution lifetime

resource declaration
    ↓
explicit persistence/ownership semantics
```

Developers should not have to annotate every value with `"ephemeral"` or `"persistent"`.

The workload should provide the natural default.

---

# 6. Application Model

The runtime should build an immutable **ApplicationDefinition** before serving real work.

Conceptually:

```text
ApplicationDefinition
│
├── metadata
├── modules
├── workloads
│   ├── triggers
│   ├── schemas/contracts
│   ├── auth metadata
│   ├── execution policies
│   └── lifetime semantics
│
├── resources
│   ├── type
│   ├── configuration requirements
│   └── ownership contracts
│
├── config/environment requirements
├── migrations/seeders metadata
├── compiled/bundled handlers
└── runtime metadata
```

The ApplicationDefinition is:

* immutable after build/load;
* reusable across many execution worlds;
* inspectable without executing arbitrary business work;
* the source of truth for runtime routing and workload topology;
* usable by CLI tooling;
* usable for generated API metadata;
* eventually usable by deployment systems such as Sakala.

The runtime should avoid hidden topology that only exists through filesystem scanning or accidental module side effects.

---

# 7. Application Root

Usai should prefer one explicit application root.

Example:

```ts
import { defineApp } from "@sakaladev/usai";

import { users } from "./users/module";
import { billing } from "./billing/module";
import { cleanup } from "./jobs/cleanup";

export default defineApp({
  modules: [
    users,
    billing,
  ],

  workloads: [
    cleanup,
  ],
});
```

The exact syntax may evolve.

The invariant is more important than the syntax:

> The application graph must be explicit and deterministic.

Usai should not require one giant file.

Large applications should be able to compose modules.

---

# 8. Modules

A module is an **organizational and ownership unit**, not automatically a runtime process, service, isolate, or security boundary.

Example:

```ts
export default defineModule({
  name: "billing",

  workloads: [
    createInvoice,
    capturePayment,
    reconcileInvoices,
  ],

  resources: [
    billingDb,
  ],
});
```

A module may also contribute project metadata such as migrations or seeders.

Example:

```ts
export default defineModule({
  name: "billing",

  migrations: "./migrations/*.sql",
  seeders: "./seeders/*.ts",

  workloads: [
    createInvoice,
    capturePayment,
  ],
});
```

Important:

```text
module != process
module != execution lifetime
module != deployment unit
module != security sandbox
```

A module is primarily a logical application grouping.

---

# 9. Workload Model

Usai should model application behavior through first-class workload types.

The initial taxonomy should distinguish at least three lifetime families.

## 9.1 Finite work

A world is created for one bounded invocation and ends when the invocation reaches a terminal state.

Examples:

* HTTP request;
* task invocation;
* cron invocation;
* queue message;
* CLI command;
* migration;
* seeder.

Conceptual lifecycle:

```text
trigger
→ fresh work world
→ execute
→ settle owned work
→ commit/finish output if applicable
→ world ends
```

## 9.2 Connection-bound work

The world naturally lives as long as a connection or stream.

Examples:

* WebSocket;
* streaming HTTP response;
* future long-lived protocol sessions.

Conceptual lifecycle:

```text
connection opens
→ world starts
→ state may remain mutable for this connection
→ connection closes/cancels
→ owned work settles
→ world ends
```

## 9.3 Persistent work

The application intentionally owns a long-running execution context.

Example:

* service.

Conceptual lifecycle:

```text
service starts
→ world starts
→ state persists intentionally
→ service drains/stops
→ world ends
```

Usai must not treat long-running execution as a failure of the model.

It is valid when the workload itself is naturally persistent.

---

# 10. HTTP Workload

HTTP should be the first mature workload, but **not the architecture around which every other workload is forced**.

The recommended application shape should be contract-aware.

Example:

```ts
import { http } from "@sakaladev/usai";
import { z } from "zod";

const Params = z.object({
  id: z.string().uuid(),
});

const User = z.object({
  id: z.string().uuid(),
  name: z.string(),
  email: z.string().email(),
});

export const getUser = http.get("/users/:id", {
  params: Params,
  response: {
    200: User,
  },
}, async (ctx) => {
  return ctx.resources.db.one(
    "select id, name, email from users where id = $1",
    [ctx.params.id],
  );
});
```

The exact schema library is not fixed by this document.

Usai should prefer compatibility with a common schema contract instead of inventing a schema system solely to own one.

The HTTP runtime should understand:

* method;
* path;
* path parameters;
* query;
* headers;
* request body;
* auth requirements;
* response contracts;
* declared errors;
* workload lifetime;
* execution policy.

---

# 11. HTTP Request Pipeline

The intended flow is:

```text
network traffic
    ↓
Usai HTTP host
    ↓
route match
    ↓
decode transport data
    ↓
boundary validation
    ↓
auth boundary if applicable
    ↓
admit execution work
    ↓
create WorkWorld
    ↓
invoke typed handler
    ↓
business logic
    ↓
response encoding / contract enforcement
    ↓
commit response
    ↓
settle ownership
    ↓
world ends
```

A key optimization and semantic property:

> Requests that fail before application work exists should not need to create a WorkWorld.

Examples:

* no matching route;
* malformed transport input;
* body schema failure;
* missing required header;
* rejected authentication before application execution.

This keeps application-world creation aligned with meaningful application work.

---

# 12. Validation Boundary

Usai should handle **boundary validation**.

Examples:

* type/shape correctness;
* required fields;
* UUID/email/enum shape;
* numeric limits declared in schema;
* path/query/header/body decoding;
* message contract validation.

The application should handle **business validation**.

Examples:

* insufficient balance;
* order cannot transition from current state;
* user lacks business permission;
* inventory rule violation;
* domain invariant.

Rule:

```text
transport/structural validation
→ runtime

business/domain validation
→ application
```

Do not turn the runtime into a domain rules engine.

---

# 13. Response Model

For ordinary contract-aware HTTP endpoints, returning data should be easy.

Example:

```ts
return user;
```

The runtime should know how to:

* encode the response;
* apply the declared response contract;
* select the declared default status where appropriate;
* generate safe error behavior.

Explicit status should remain possible.

Example shape:

```ts
return http.created(user);
```

or another similarly small primitive.

The exact syntax is open.

The runtime should avoid unnecessary duplicate validation and serialization passes when a schema-aware encoder can combine correctness and encoding efficiently.

---

# 14. Error Model

Errors should be first-class application contracts.

Expected developer experience:

```ts
throw errors.notFound("user not found");
```

The runtime should:

* map known application errors to stable transport responses;
* sanitize unexpected internal failures;
* preserve full internal diagnostic context in logs/traces;
* never leak internal stack traces by default in production.

Endpoints should be able to declare known errors so documentation and generated clients can understand them.

Unexpected errors remain runtime failures, not part of the normal contract unless explicitly declared.

---

# 15. Raw HTTP Escape Hatch

Usai must not become a prison.

Some workloads require access to lower-level transport behavior.

Examples:

* webhook signature verification over exact bytes;
* non-JSON body formats;
* streaming;
* proxy behavior;
* custom multipart handling;
* unusual response formats.

Usai should provide an explicit raw or low-level HTTP path.

Conceptually:

```ts
http.raw("/stripe/webhook", async (ctx) => {
  const body = await ctx.request.bytes();

  // verify signature from exact bytes

  return new Response("ok");
});
```

The runtime must still own:

* workload lifetime;
* cancellation;
* ownership;
* world destruction;
* resource semantics.

But when using raw transport:

* automatic schema validation may be unavailable;
* generated OpenAPI may be partial/opaque;
* client generation may be limited.

The trade-off must be explicit.

---

# 16. Task Workload

A task is finite work with its own independent lifecycle.

Example:

```ts
export const sendReceipt = task("send-receipt", {
  input: SendReceipt,
}, async (ctx) => {
  // ...
});
```

Tasks are important because Usai should not allow accidental detached work.

The programming model must distinguish:

```text
work owned by current world
```

from:

```text
work transferred to another lifetime
```

Conceptually:

```ts
await ctx.tasks.invoke(validateOrder, input);
```

means the current work still owns/waits for the child operation.

Whereas:

```ts
await ctx.tasks.dispatch(sendReceipt, input);
```

means ownership is transferred to a task runtime and the parent may complete independently.

The exact API names may change.

The semantic distinction must remain.

---

# 17. No Implicit Detached Work

This should be a core Usai rule:

> **Asynchronous work must either be owned by the current workload or explicitly transferred to another lifetime.**

Example of code Usai should detect, cancel, reject, or strongly warn about:

```ts
http.post("/orders", async () => {
  setTimeout(sendEmail, 1000);
  return { ok: true };
});
```

The runtime should be able to explain:

```text
HTTP work ended with live asynchronous work.

The request no longer owns execution after the response completes.

Use:
- task() for independent finite work
- cron() for scheduled work
- service() for intentional long-running work
```

This is not merely linting.

It is part of the lifecycle model.

---

# 18. Cron Workload

Cron should be runtime-declared scheduled finite work.

Example:

```ts
export const cleanup = cron("cleanup", {
  schedule: "0 3 * * *",
  timeout: "5m",
  overlap: "skip",
}, async (ctx) => {
  await ctx.resources.db.execute(
    "delete from sessions where expires_at < now()",
  );
});
```

Each invocation should receive an independent finite world.

The scheduler may live for the runtime/application lifetime.

The invocation should not.

Usai should eventually support clear overlap behavior such as:

* allow;
* skip;
* queue;
* replace/cancel previous where safe.

Do not overbuild scheduler semantics before real workloads require them.

---

# 19. Queue / Message Workload

A queue consumer should separate persistent consumer infrastructure from per-message application state.

Conceptually:

```text
persistent queue connection/consumer
    │
    ├── message A → fresh world → end
    ├── message B → fresh world → end
    └── message C → fresh world → end
```

Developer shape:

```ts
export const orders = queue.consume("orders", {
  message: OrderEvent,
  concurrency: 16,
}, async (ctx) => {
  await processOrder(ctx.message);
});
```

The runtime should know:

* message contract;
* message lifetime;
* concurrency policy;
* retry/dead-letter policy when introduced;
* resource ownership.

Do not default to one long-lived mutable application world simply because the queue connection itself is persistent.

---

# 20. WebSocket / Connection Workload

WebSocket state naturally belongs to connection lifetime.

Example:

```ts
export const chat = socket("/chat", {
  incoming: ChatMessage,
  outgoing: ChatEvent,
}, {
  async open(ctx) {
    ctx.state.user = await authenticate(ctx.request);
  },

  async message(ctx) {
    ctx.state.messages = (ctx.state.messages ?? 0) + 1;
  },
});
```

The runtime should permit connection-local mutable state.

That state should end when the connection ends.

If state must survive connections, it must move into an explicit persistent resource or durable capability.

Do not silently promote connection state into process-global state.

---

# 21. Stream Workload

Streaming HTTP or similar bounded streams should keep their world alive for the stream lifetime.

Example:

```ts
export const events = http.stream("/events", async (ctx, stream) => {
  while (!ctx.signal.aborted) {
    await stream.send(await nextEvent());
  }
});
```

The runtime must understand:

```text
headers sent
!=
work complete
```

The world ends when the stream reaches its natural terminal state.

---

# 22. Service Workload

Long-running work must be first-class.

Example:

```ts
export const reconciler = service("reconciler", async (ctx) => {
  const state = new Map();

  while (!ctx.signal.aborted) {
    await reconcile(state);
    await ctx.sleep("5s");
  }
});
```

Here the mutable state intentionally survives because the service itself remains alive.

Service lifecycle should support:

* start;
* readiness;
* cancellation;
* graceful drain;
* stop;
* crash/restart policy later.

Usai must not market itself as "anti long-running".

The real rule is:

> **Long-running state should exist because long-running work requires it, not because the process happened to stay alive.**

---

# 23. Resource Model

Resources represent state or capability whose lifetime is intentionally longer than one finite work world.

Examples:

* PostgreSQL pool;
* Redis client;
* runtime-local cache;
* object storage;
* secrets/config provider;
* queue infrastructure;
* future durable resources.

Resources must have explicit ownership semantics.

The runtime should distinguish:

```text
resource manager lifetime
lease lifetime
external operation lifetime
workload lifetime
```

These are not automatically equal.

---

# 24. PostgreSQL Contract

PostgreSQL should be the first serious persistent capability.

Research-established behavior should guide production semantics.

## Normal completion

```text
world borrows connection
→ query reaches terminal state
→ connection safe
→ return to pool
```

## Ordinary database error

If protocol state is known and terminal:

```text
error
→ connection state known
→ return to pool
```

## Cooperative cancellation / timeout

```text
cancellation requested
→ database cancellation sent
→ original operation reaches known terminal state
→ connection reusable only after terminal proof
```

## Ambiguous hard abandonment

```text
world disappears while external operation is ambiguous
→ never assume connection safe
→ quarantine/remove/replace as required
```

Rule:

> **World death is not proof that an external resource is reusable.**

---

# 25. Persistent Application State

Usai should not initially expose arbitrary process-global mutable objects as the primary persistence model.

Avoid making this the default:

```ts
persistent(() => new Map())
```

because it creates unresolved semantics around:

* concurrency;
* thread sharing;
* revision sharing;
* runtime restart;
* durability;
* replication;
* serialization;
* multi-node behavior.

Instead, persistence should initially be represented by explicit resources with clear contracts.

Examples:

```ts
postgres("main", ...)
cache.local("catalog", ...)
```

A runtime-local cache may explicitly mean:

```text
shared across worlds
local to runtime/revision
not durable
may disappear on restart
```

Different persistence classes should be different resource contracts, not one magical shared-object primitive.

---

# 26. Configuration Philosophy

`usai.config.ts` is important, but it must not become a landfill.

Its purpose is primarily to map a developer's project structure into the Usai application/build model.

A useful rule:

> **Config should map structure, not invent semantics.**

Semantics belong in workload/resource declarations whenever possible.

Configuration should be:

* typed;
* declarative;
* deterministic;
* inspectable;
* small by default.

A new project should ideally work with zero or near-zero config.

Convention first.

Override without friction.

---

# 27. Project Structure

Usai should not force a domain architecture.

Minimal project:

```text
my-app/
├── src/
│   └── app.ts
├── package.json
├── tsconfig.json
└── usai.config.ts
```

A larger project may use:

```text
src/
├── app.ts
├── users/
│   ├── module.ts
│   ├── routes.ts
│   ├── service.ts
│   ├── migrations/
│   └── seeders/
├── billing/
│   ├── module.ts
│   ├── routes.ts
│   ├── migrations/
│   └── seeders/
└── shared/
```

Another developer may prefer:

```text
src/
├── app.ts
├── routes/
├── services/
├── repositories/
└── jobs/

migrations/
seeders/
```

Both should be valid.

Usai should care about the resulting ApplicationDefinition, not arbitrary folder aesthetics.

---

# 28. Project Configuration

Example:

```ts
import { defineConfig } from "@sakaladev/usai/config";

export default defineConfig({
  app: "./src/app.ts",

  database: {
    migrations: {
      include: [
        "./src/**/migrations/*.sql",
      ],
    },

    seeders: {
      include: [
        "./src/**/seeders/*.ts",
      ],
    },
  },
});
```

A developer who prefers centralized files may use:

```ts
export default defineConfig({
  database: {
    migrations: {
      include: ["./migrations/*.sql"],
    },

    seeders: {
      include: ["./seeders/*.ts"],
    },
  },
});
```

Project configuration should support organization without changing application semantics.

---

# 29. Configuration Precedence

Avoid an override system that becomes impossible to reason about.

A simple intended precedence:

```text
explicit module/project declaration
    ↓
root project configuration
    ↓
built-in convention/default
```

Operational CLI flags may override one invocation where appropriate, but must not silently redefine application semantics.

Usai should provide:

```bash
usai config
```

to show the effective configuration and where each value came from.

Example:

```text
Application entry
  ./src/app.ts
  source: default

Migrations
  ./src/**/migrations/*.sql
  source: usai.config.ts

Seeders
  ./src/**/seeders/*.ts
  source: usai.config.ts
```

This command is important if configuration is flexible.

---

# 30. Build-Time vs Runtime Configuration

Do not conflate application definition with one deployment instance.

`usai.config.ts` should mostly define:

* source/app entry;
* project discovery;
* build behavior;
* migration/seeder discovery;
* generated output policy;
* other stable project metadata.

Deployment-specific settings such as:

* listen port;
* server concurrency;
* CPU limits;
* memory limits;
* secrets;
* instance-level tuning;

should primarily come from runtime/deployment configuration, environment, CLI invocation, or an orchestrator.

Rule:

```text
application definition
!=
deployment instance
```

---

# 31. Deterministic Configuration

Although configuration may use TypeScript syntax for type safety, its semantics should remain declarative.

Avoid configuration that arbitrarily:

* calls remote APIs;
* queries production databases;
* depends on current wall time;
* mutates external state;
* runs uncontrolled shell commands during definition.

Ideal property:

```text
same source
+ same declared build inputs
→ same logical ApplicationDefinition
```

Perfect byte reproducibility may be a later engineering target, but avoid designing nondeterminism into the project model.

---

# 32. Environment and Secrets

Avoid making raw `process.env` the only application configuration model.

The application should be able to declare required configuration.

Conceptually:

```ts
const config = env({
  DATABASE_URL: env.url(),
  JWT_SECRET: env.secret(),
  APP_ENV: env.enum(["development", "production"]),
});
```

Benefits:

* startup validation;
* typed access;
* secret classification;
* deployment introspection;
* better error messages;
* future Sakala integration.

Missing required configuration should fail application activation clearly rather than failing on the first request.

---

# 33. Migrations

Migrations may be centralized or colocated by module.

Usai should not require an ORM.

SQL migrations are sufficient for the initial model.

Examples:

```text
migrations/
001_users.sql
002_orders.sql
```

or:

```text
src/users/migrations/001_users.sql
src/billing/migrations/001_invoices.sql
```

The CLI should eventually support:

```bash
usai db migrate
```

Migrations themselves are finite work and should execute under explicit runtime ownership rather than ad-hoc scripts.

Do not automatically run production migrations on every runtime startup unless an explicit product decision later justifies that behavior.

---

# 34. Seeders

Seeders should support the same flexible discovery model.

Example:

```text
src/users/seeders/dev.ts
src/catalog/seeders/demo.ts
```

Useful commands may include:

```bash
usai db seed
usai db seed users
usai db seed demo
```

Seeders are finite application work.

They should receive controlled access to declared resources.

They should not implicitly become part of normal application startup.

---

# 35. User-Defined Commands

Usai may support finite developer/operator commands.

Example:

```ts
export const reconcileUsers = command(
  "reconcile-users",
  async (ctx) => {
    // finite application work
  },
);
```

Invocation:

```bash
usai app reconcile-users
```

Natural lifetime:

```text
command invocation
→ fresh world
→ complete
→ world ends
```

This primitive can eventually provide a clean home for maintenance tasks that would otherwise become random scripts.

Do not implement this before the core workload model is ready.

---

# 36. Testing Model

Testing should operate on the same application model as production.

Conceptually:

```ts
import { testApp } from "@sakaladev/usai/test";

const app = await testApp(MyApp);
```

HTTP:

```ts
const response = await app.http.post("/users", {
  body: {
    name: "Ayu",
  },
});
```

Task:

```ts
await app.task("send-receipt").invoke({
  orderId: "...",
});
```

Cron:

```ts
await app.cron("cleanup").run();
```

Lifecycle-specific tests should be easy.

Example:

```ts
await app.http.get("/mutate");
const response = await app.http.get("/read");

expect(response.body.counter).toBe(0);
```

Testing should not require real wall-clock cron scheduling or external HTTP when a deterministic in-process harness can exercise the same ApplicationDefinition.

---

# 37. CLI North Star

The CLI should eventually have a coherent surface around the application model.

Core:

```bash
usai dev
usai build
usai run
```

Introspection:

```bash
usai inspect
usai graph
usai config
```

Database:

```bash
usai db migrate
usai db seed
```

Generation:

```bash
usai generate openapi
usai generate client
```

Testing:

```bash
usai test
```

Do not implement commands only because they appear in this list.

Each command should exist when the underlying model is mature enough to justify it.

---

# 38. `usai dev`

`usai dev` should become the emotional center of the early developer experience.

Example output:

```text
Usai

Application  billing
Revision     dev

HTTP
  GET   /users/:id
  POST  /orders

Tasks
  send-receipt

Cron
  cleanup          0 * * * *

Resources
  postgres/main    ready

App       http://localhost:3000
API Docs  http://localhost:3000/_usai/docs
Inspect   http://localhost:3000/_usai/inspect
```

The development server should:

* detect project changes;
* rebuild/reload safely;
* never corrupt active ownership;
* provide clear errors;
* preserve deterministic application topology;
* keep tooling fast enough for ordinary development.

Development reload semantics must be designed, not hacked around process restart forever.

---

# 39. `usai inspect`

`usai inspect` should answer:

> **What application did Usai actually understand?**

Example:

```text
Application: billing

Modules
  users
  billing

HTTP
  GET /users/:id
    lifetime: request
    response: User

  POST /orders
    lifetime: request
    resources:
      postgres/main
    dispatches:
      send-receipt

Tasks
  send-receipt
    lifetime: task

Cron
  cleanup
    schedule: 0 * * * *
    lifetime: invocation

Resources
  postgres/main
    lifetime: runtime
    lease: per work
```

This command should read the ApplicationDefinition, not reconstruct a separate model.

---

# 40. `usai graph`

`usai graph` should expose the workload/resource/ownership graph.

Example:

```text
POST /orders [request]
   │
   ├── postgres/main [lease]
   │
   └── dispatch
          ↓
      send-receipt [task]
          │
          └── postgres/main [lease]

cleanup [cron]
   └── postgres/main [lease]

ledger-sync [service]
   └── postgres/main [lease]
```

Eventually the graph may expose lifetime relationships.

This is not decorative documentation.

It should reflect relationships the runtime actually uses.

---

# 41. Automatic API Documentation

If the runtime knows:

* HTTP methods;
* paths;
* path/query/header contracts;
* request bodies;
* responses;
* errors;
* auth metadata;

then generating OpenAPI is a natural product capability.

Usai should eventually support:

```bash
usai generate openapi
```

and local interactive docs in development.

Generated documentation must come from the same ApplicationDefinition used for execution.

Avoid a separate annotation/config system that can drift from runtime behavior.

---

# 42. Generated Clients

Generated clients are a future capability that becomes possible once the application contract is stable.

Example:

```bash
usai generate client --lang typescript
```

Do not prioritize multi-language generation early.

A high-quality TypeScript client is more useful than five weak generators.

---

# 43. Schema Strategy

Usai should not immediately invent a new schema language.

Prefer compatibility with existing TypeScript schema ecosystems through a common contract where practical.

The requirements are:

* runtime validation;
* TypeScript inference where possible;
* enough metadata for OpenAPI/JSON Schema generation where supported;
* predictable performance;
* no forced dependency on one validator library if avoidable.

If a built-in Usai schema library is ever introduced, it must solve a concrete problem that existing schema ecosystems cannot solve cleanly.

---

# 44. Observability

Because Usai understands workloads and lifetimes, observability should be lifecycle-aware.

The runtime should eventually expose structured records for:

* application revision;
* workload name/type;
* work/world identity;
* request/message/task identity;
* resource leases;
* external operations;
* cancellation;
* abandonment;
* response commit rights;
* ownership return to baseline.

Example conceptual trace:

```text
Work #1234
type       HTTP
route      POST /orders
world      82
revision   a8cd19
duration   23ms

resources
  postgres/main
    leased    4ms
    returned  terminal

children
  send-receipt
    ownership transferred
```

Abnormal path:

```text
HTTP world #82
response rights revoked

SQL operation #441
continued after world abandonment

connection
quarantined → replaced
```

Disabled detailed tracing must be genuinely cheap.

Research already demonstrated that "disabled but still allocating" is an avoidable runtime defect.

---

# 45. Lifecycle-Aware Errors and Diagnostics

Error messages should teach the programming model.

Bad:

```text
invalid operation
```

Better:

```text
HTTP work ended with a live interval.

The request lifetime ended after the response completed.
The interval cannot remain owned by this world.

Use:
  task()    for detached finite work
  cron()    for scheduled work
  service() for intentional long-running work
```

Usai should use its knowledge of lifecycle semantics to give actionable feedback.

This can become a meaningful DX advantage.

---

# 46. Runtime Architecture Model

The production runtime should preserve the following lifetime separation.

## Persistent Runtime State

Examples:

* HTTP/network listener;
* scheduler;
* engine/runtime infrastructure;
* observability;
* resource managers;
* connection pools.

## Immutable Application Definition

Examples:

* compiled code;
* route/workload metadata;
* schemas;
* config requirements;
* dependency metadata;
* preinitialized application representation;
* engine pre-instantiation state that naturally belongs to the definition.

## Execution World

Examples:

* request/message/task/connection state;
* mutable application globals;
* temporary allocations;
* auth/tenant context;
* resource leases.

## Resource Lease

Temporary ownership of a persistent resource.

## External Operation

Work that may continue outside the execution world that initiated it.

## Physical Residency

An implementation detail separate from semantic lifetime.

Do not collapse these layers merely because a convenient implementation makes them adjacent.

---

# 47. Implementation Lessons Inherited from Research

The production code should not reintroduce known costs that were unrelated to the desired semantics.

Avoid:

* process-lifetime history structures scanned per request;
* unbounded retained bookkeeping for dead work;
* rebuilding application-definition objects per world;
* rebuilding linker/import/export metadata per request without necessity;
* eager formatting/allocation for disabled observability;
* resolving all exported functions eagerly when lazy resolution is sufficient;
* rewriting whole kept-resident memory regions when pagemap-aware reset can safely avoid it.

Preserve the successful direction:

* bounded live ownership state;
* application-definition lifetime reuse;
* pooling + copy-on-write where the chosen engine representation supports it;
* lazy resolution where appropriate;
* pagemap-aware reset where supported and justified;
* explicit capability ownership.

These are implementation lessons, not sacred code.

A cleaner production mechanism is preferred if it preserves the same semantics.

---

# 48. Repository Structure

Start small.

Suggested initial structure:

```text
crates/
  usai-runtime/
  usai-cli/

packages/
  usai/
  create-usai/

examples/
  hello/
  postgres/
  lifecycle/

docs/
tests/
```

Do not prematurely create a crate for every semantic noun.

Semantic boundaries do not automatically require Cargo package boundaries.

Split crates only when there is a concrete reason involving:

* dependencies;
* compilation boundaries;
* API stability;
* reuse;
* ownership;
* testing;
* build performance.

---

# 49. Developer Surface

The initial public developer language remains TypeScript.

The runtime host is expected to remain Rust unless production evidence changes that decision.

The build pipeline may use JavaScript/Wasm/component artifacts, but the engine representation is not part of the public programming model.

Do not expose:

* Wasmtime-specific options;
* memory reservation internals;
* pooling slot internals;
* pagemap toggles;
* engine implementation knobs;

as ordinary application configuration.

Those belong to runtime internals or advanced operator/debug surfaces.

---

# 50. Public API Stability

Do not freeze public API too early.

Before the first developer preview:

* prioritize semantic clarity;
* allow breaking changes;
* prefer explicit experimental namespaces where needed;
* avoid compatibility promises that prevent correcting a bad programming model.

Once an API becomes documented as stable, treat compatibility as a real cost.

Irreversible public API decisions are one of the cases where the agent should stop and request design review.

---

# 51. Development Mode

This repository operates in normal software-engineering mode by default:

```text
design
→ implement
→ test
→ benchmark
→ profile
→ improve
→ regression test
→ release
```

Ordinary development does not require:

* preregistration;
* freeze manifests;
* one-shot claims;
* canonical evidence archives;
* external adjudication.

Benchmarks are normal engineering tools.

Formal research is reserved for decision-critical uncertainty.

---

# 52. When to Return to Formal Research

A formal experiment may be justified when a question:

1. can materially change the runtime architecture;
2. cannot be answered reliably by normal tests/profiling/comparison;
3. is vulnerable to benchmark retuning or hindsight bias;
4. has enough cost/importance to justify methodological ceremony.

Possible examples:

* major JS/Wasm engine representation change;
* multi-core execution architecture;
* security isolation versus density trade-off;
* runtime-local state model with serious concurrency implications;
* memory reservation policy determining many-app density;
* a fundamental scheduling model choice;
* cross-node persistence/lifecycle semantics.

Examples that do **not** automatically need research:

* one slow function;
* one allocation hotspot;
* routing implementation choice;
* CLI formatting;
* local refactor;
* library selection with clear practical winner.

Profile first.

Research only when engineering evidence is insufficient.

---

# 53. Product Milestones

The milestones below are intentionally ordered so that the programming model is tested across multiple lifetimes before broad feature expansion.

## D0 — Production Repository Foundation

Create:

* Rust workspace;
* TypeScript workspace;
* CI;
* formatting/lint/test commands;
* docs structure;
* examples;
* build/dev scripts;
* research-reference document.

Acceptance:

* clean clone builds;
* tests pass with documented commands;
* no runtime/build dependency on the research repository;
* repository is clearly production-oriented, not experiment-oriented.

---

## D1 — Application Definition + Lifecycle Core

Implement production concepts for:

* Runtime;
* ApplicationDefinition;
* Module;
* Workload abstraction;
* WorkWorld / ExecutionContext;
* Resource;
* ResourceLease;
* ExternalOperation ownership.

Acceptance:

* immutable application definition can create multiple worlds;
* world A mutable application state is not visible in world B;
* persistent runtime infrastructure survives world destruction;
* ownership returns to baseline;
* abnormal world destruction does not leave stale execution rights;
* lifecycle semantics are testable without HTTP.

This milestone is more important than routing.

---

## D2 — HTTP Contract Workload

Implement the first real workload end-to-end.

Include:

* routing;
* params/query/headers/body;
* schema boundary;
* typed context;
* response encoding;
* declared application errors;
* raw HTTP escape hatch;
* cancellation signal;
* real HTTP server.

Acceptance:

* contract-aware endpoint works end-to-end;
* invalid boundary input can fail before WorkWorld creation where appropriate;
* fresh world per finite HTTP request;
* correct response ownership;
* concurrent requests do not leak mutable application state.

---

## D3 — Developer Loop

Implement:

```bash
usai dev
usai inspect
usai config
```

Include:

* project discovery;
* config defaults;
* rebuild/reload;
* readable diagnostics;
* application topology display.

Acceptance:

* new developer can clone an example and run it;
* editing a handler reloads safely;
* invalid application definition fails clearly;
* effective config can be inspected;
* application topology matches actual runtime behavior.

This is the milestone where Usai should begin to feel like a product.

---

## D4 — Tasks + Explicit Ownership Transfer

Implement:

* task definition;
* task input contract;
* owned invocation;
* detached dispatch / ownership transfer;
* no implicit detached work.

Acceptance:

* HTTP request can dispatch a task and end safely;
* task receives a separate world;
* parent state does not leak to task world;
* ownership transfer is explicit and observable;
* dangling async work in finite worlds is rejected, cancelled, or diagnosed clearly.

This milestone validates that Usai is not merely an HTTP framework.

---

## D5 — Cron + Commands

Implement:

* cron declarations;
* finite invocation worlds;
* basic overlap policy;
* user-defined commands;
* deterministic test invocation.

Acceptance:

* cron runs in fresh worlds;
* command runs in a fresh finite world;
* neither requires a permanent mutable application process;
* tests can invoke both without waiting for wall clock.

---

## D6 — PostgreSQL Capability

Implement production PostgreSQL resource ownership.

Include:

* persistent pool/resource manager;
* per-work leases;
* normal completion;
* SQL errors;
* cancellation;
* timeout;
* ambiguous abandonment;
* quarantine/replacement;
* recovery.

Acceptance:

* connection reuse only after terminal knowledge;
* no connection-state leak across worlds;
* cancellation behavior covered;
* abandoned-world behavior covered;
* recovery request succeeds after abnormal cases.

---

## D7 — Project Model: Modules, Migrations, Seeders, Config

Implement mature project organization support.

Include:

* module contributions;
* migration discovery;
* seeder discovery;
* config overrides;
* effective config introspection;
* typed environment requirements.

Acceptance:

* centralized and colocated project layouts both work;
* project organization does not change runtime semantics;
* module metadata composes deterministically;
* config remains declarative and inspectable;
* missing environment requirements fail before serving work.

---

## D8 — API Metadata + OpenAPI

Build API docs from the actual HTTP workload definition.

Include:

* OpenAPI generation;
* development docs UI;
* stable error/response metadata;
* schema metadata conversion where supported.

Acceptance:

* docs match runtime behavior;
* no duplicate endpoint-definition system;
* raw endpoints are represented explicitly as partially/fully opaque where necessary.

---

## D9 — Service Workload

Implement intentional long-running work.

Include:

* start;
* signal/cancel;
* drain;
* stop;
* restart policy later.

Acceptance:

* service state persists intentionally for service lifetime;
* service lifetime is separate from runtime lifetime;
* graceful stop returns ownership to baseline;
* service API does not weaken finite-work isolation semantics.

---

## D10 — Queue / Message Workload

Implement persistent consumer infrastructure plus per-message execution worlds.

Acceptance:

* message contract validation;
* bounded concurrency;
* per-message world;
* no mutable state leak across messages;
* retry/error semantics explicitly defined;
* resource reuse follows ownership rules.

---

## D11 — WebSocket + Stream Workloads

Implement connection-bound worlds.

Acceptance:

* connection-local mutable state survives messages;
* state disappears when connection ends;
* stream lifetime is distinct from ordinary HTTP request completion;
* cancellation/drain behavior is correct;
* no accidental process-global connection state.

---

## D12 — Observability + Graph

Implement:

* structured logs;
* metrics;
* lifecycle tracing;
* `usai graph`;
* workload/resource ownership inspection.

Acceptance:

* observability derives from runtime truth;
* detailed tracing can be disabled cheaply;
* ownership/lifetime failures are diagnosable.

---

## D13 — Production Hardening

Exercise:

* long-duration soak;
* sustained concurrency;
* overload/backpressure;
* graceful shutdown;
* forced shutdown;
* crash/restart recovery;
* bounded memory behavior;
* database loss/recovery;
* malformed artifacts;
* artifact/version compatibility;
* revision activation/draining;
* resource limits;
* security/threat model;
* upgrade/rollback.

Do not call Usai production-ready before this work produces evidence.

---

## D14 — Developer Preview / Alpha

A developer preview is ready when:

* `usai dev` is coherent;
* `usai build` produces a runnable artifact;
* `usai run` serves the artifact;
* HTTP contract model works;
* task/cron proves multiple finite lifetimes;
* PostgreSQL ownership is correct;
* config/project model is usable;
* API docs are generated from runtime truth;
* lifecycle integration tests are green;
* observability is sufficient to debug failures;
* docs let a new developer build a real application.

The release may explicitly be alpha.

---

## D15 — Sakala Integration

Only after the standalone runtime is coherent.

Expose a stable local control surface suitable for orchestrators.

Likely general operations:

```text
install revision
activate revision
inspect
health
drain revision
stop
remove revision
```

Keep the control protocol generally useful.

Sakala should become a first-class integration, not the definition of Usai.

---

# 54. Non-Goals Before Alpha

Do not build unless earlier milestones prove they are required:

* new programming language;
* ORM;
* full Node.js compatibility;
* native addon compatibility;
* plugin marketplace;
* package registry;
* distributed scheduler;
* multi-region runtime;
* microVM sandbox;
* custom JavaScript engine;
* arbitrary durable-object model;
* broad Redis/Kafka/etc capability catalog;
* framework-level MVC/DI system;
* Sakala-specific runtime coupling.

Do not build completeness for its own sake.

---

# 55. Agent Autonomy Rules

An implementation agent may make ordinary local engineering decisions without asking for approval.

Prefer:

* the smaller coherent design;
* fewer abstractions;
* explicit ownership;
* explicit lifetime boundaries;
* testability;
* deterministic application models;
* minimal public API surface.

The agent should stop and surface the decision when a change would:

* contradict an established lifecycle contract;
* materially alter the runtime thesis;
* introduce a new engine/runtime representation;
* introduce a major security boundary;
* create irreversible public API compatibility;
* create implicit detached work;
* introduce a persistence model whose concurrency/durability semantics are unclear;
* require decision-critical performance assumptions not supported by evidence.

Do not stop merely because implementation is difficult.

---

# 56. Design Questions Intentionally Left Open

These questions should not block D0/D1, but they should not be silently decided by accident.

## Schema integration

* Standard Schema compatibility?
* direct JSON Schema?
* built-in minimal schema library later?
* how much metadata can safely be extracted from third-party schemas?

## Response API

* return plain values?
* `http.created(...)` helpers?
* explicit response object?
* how response contracts map to streaming/raw responses?

## Authentication model

* reusable auth workload boundary?
* per-route auth declaration?
* how auth context enters WorkWorld?
* how much auth metadata goes into OpenAPI?

## Application artifact

* exact bundle/component format;
* reproducibility guarantees;
* version compatibility policy.

## Development reload

* whole application definition replacement?
* per-module rebuild?
* draining old revision during dev?
* persistent resources reused across reload?

## Runtime-local state

* whether a safe explicit runtime-local state primitive is needed beyond cache/resources.

## Revision lifecycle

* install/activate/drain/remove exact contract.

## Multi-core model

* one runtime with shared resources?
* sharded engines?
* scheduling topology?
* when to measure and formally research.

## Security boundary

* what isolation guarantees Usai promises;
* what it explicitly does not promise.

These should become ADRs, design notes, or formal experiments when they become decision-critical.

---

# 57. Definition of Product Success

Usai succeeds when developers can express multiple kinds of backend work using one coherent programming model, and the runtime gives each kind of work the correct lifetime without forcing unrelated state to live together.

A representative application should be able to contain:

```text
HTTP
  POST /orders
  lifetime: request

Task
  send-receipt
  lifetime: task

Cron
  cleanup-expired
  lifetime: invocation

Queue
  payment-events
  lifetime: message

WebSocket
  /notifications
  lifetime: connection

Service
  ledger-sync
  lifetime: service

Resources
  postgres/main
  lifetime: runtime/resource manager
```

The developer should not manually build seven process-lifecycle strategies to achieve that.

The runtime should understand:

* what work exists;
* what owns it;
* what it may access;
* what can outlive it;
* when it is safe to end;
* when a resource is safe to reuse;
* what contract crosses the application boundary.

The product experience should eventually feel like:

```text
declare work
declare resources
write business logic
run Usai
```

not:

```text
manually coordinate process lifetime
manually clean request globals
hope detached promises finish
hope pooled resources are safe
write separate docs/configs that drift from code
```

---

# 58. The Core Promise

Usai should eventually make one promise developers can understand:

> **Write work according to its natural lifetime. Usai will keep only what deserves to stay alive.**

That is the product.

The research proved enough to justify trying to build it.

The production repository now has to earn the rest.
