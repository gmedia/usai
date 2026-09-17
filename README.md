# Usai

> **A program should live only as long as its work requires.**

Usai is a workload-native application runtime that keeps expensive runtime
infrastructure alive while making application execution ephemeral by default.

Usai is an independent open-source project in the Sakala ecosystem. It is
designed to remain useful on its own: applications must be able to develop,
build, and run on Usai without requiring Sakala or another deployment platform.

## Status

**Early development. Not production-ready yet.**

The research phase established enough evidence to justify building a real
runtime. This repository is the clean production-oriented implementation.

The historical research repository remains separate and should be treated as
evidence and technical reference, not as the production codebase:

- Research repository: `HasanH47/usai`
- Production repository: `gmedia/usai`

The production runtime should inherit proven contracts and lessons from the
research, not its experiment-oriented repository structure.

## Why Usai?

Most backend runtimes naturally keep the application process and its mutable
application world alive for a long time:

```text
start
→ bootstrap
→ request
→ request
→ request
→ ...
```

That model is correct for many workloads. Usai explores a different default:

```text
persistent runtime
        +
immutable application definition
        +
fresh execution world per unit of work
```

The important distinction is not "persistent versus ephemeral".

It is:

> **What is the natural lifetime of each piece of state and each resource?**

A database connection may deserve to outlive a request.

Mutable request state usually does not.

An immutable application definition may serve thousands of executions.

An external operation may outlive the execution world that started it.

Usai makes those lifetime boundaries explicit.

## Core model

Usai separates three primary layers.

### Persistent Runtime State

Long-lived infrastructure that is expensive or nonsensical to recreate for
every request:

- HTTP/network listeners
- scheduler/runtime infrastructure
- engine and allocation infrastructure
- observability
- connection/resource managers
- runtime-owned capabilities

### Immutable Application Definition

Reusable application state that belongs to a deployment or revision:

- compiled/bundled code
- routes
- schemas and metadata
- configuration metadata
- dependency metadata
- preinitialized application representation

### Ephemeral Execution World

Mutable state whose natural lifetime is one unit of work:

- request state
- auth/user/tenant context
- mutable application globals
- temporary objects
- request-scoped dependencies
- resource leases

When the work is complete, the world ends.

The runtime does not.

## Lifetime principles

Usai is built around a few rules:

> **Persistent runtime does not imply persistent application state.**

> **Ephemeral by default; persistent by intent.**

> **Nothing should share a lifetime merely because it shares a process.**

> **Death is not cleanup. Ownership is cleanup.**

Destroying an execution world does not automatically mean an external
operation has terminated or that a resource is safe to reuse. Resource reuse
requires explicit ownership and terminal-state rules.

## Try it

```bash
pnpm install
cargo run -p usai-cli -- dev --root examples/hello
curl http://localhost:3000/hello/world
```

`examples/hello/src/app.ts`:

```ts
import { defineApp, http, errors } from "usai";
import { z } from "zod";

const Params = z.object({ name: z.string().min(1).max(40) });
const Greeting = z.object({ hello: z.string() });

export const hello = http.get("/hello/:name", { params: Params, response: Greeting }, async (ctx) => {
  if (ctx.params.name === "nobody") throw errors.notFound("nobody is not here");
  return { hello: ctx.params.name };
});

export default defineApp({ name: "hello", workloads: [hello] });
```

Each request runs in a fresh execution world; invalid params are rejected
before a world exists; `usai inspect` shows exactly what the runtime
understood. See `docs/STATUS.md` for what is implemented and what is not.

The developer surface is TypeScript and will change before the first
developer preview.

## Current implementation direction

The production implementation starts from the architecture that survived the
research program:

```text
Developer surface     TypeScript
Host runtime          Rust
Application build     TypeScript → bundled application artifact
Runtime infrastructure persistent
Application state     ephemeral by default
Persistent state      explicit
```

The exact JavaScript/Wasm engine representation is an implementation decision,
not part of the public philosophy. It may change if a better representation
preserves the same semantics and improves the total system.

## Research foundation

The research repository tested the runtime model through disposable-world
semantics, asynchronous suspension, cancellation and reclamation, persistent
PostgreSQL capabilities, real HTTP composition, simultaneous execution worlds,
and concurrency economics.

The latest sealed research checkpoint is **EXP-012B**.

The important production lesson is not a benchmark slogan. It is that
ephemeral execution can remain economically viable when the implementation
does not pay work that the semantics never required.

Research evidence must not be rewritten as marketing claims. Production
readiness, multi-core scaling, long-duration reliability, security boundaries,
many-application density, ecosystem compatibility, and operational behavior
remain engineering and validation work.

## Scope

The first production track focuses on ordinary backend workloads:

- REST APIs
- CRUD/business backends
- webhooks
- internal applications
- small and medium services
- bursty or highly idle services
- eventually, shared multi-application runtimes

This is a proving ground, not a permanent product-size restriction.

Naturally persistent workloads such as long-lived socket servers, queue
consumers, brokers, databases, proxies, or inference servers should not be
forced into request-shaped lifetimes.

## Non-goals for the initial runtime

Usai is not currently trying to build:

- a new programming language
- a full Node.js compatibility layer
- an ORM
- a distributed scheduler
- a multi-region platform
- a custom JavaScript engine
- a package registry
- a plugin marketplace
- a deployment platform

Those may become relevant someday, but none are prerequisites for proving that
the runtime itself is useful.

## Sakala ecosystem

Usai is an independent project in the Sakala open-source ecosystem.

Sakala may become a first-class deployment integration for Usai, but Usai must
remain usable independently and must not require Sakala at runtime or during
local development.

The two projects may integrate deeply without sharing a forced technical
lifetime.

## Development philosophy

This repository is now normal software engineering.

Use:

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

Formal research experiments are reserved for decision-critical questions that
ordinary implementation, testing, profiling, or benchmarking cannot answer
reliably.

The goal is no longer to reproduce the research harness.

The goal is to turn the evidence into a runtime developers can actually use.

## License

Licensed under the Apache License, Version 2.0. See [LICENSE](LICENSE).
