# Lifecycle Contracts

> Binding runtime invariants, phrased so code and tests can be checked against them.

These are the rules the production runtime must uphold. They come from `GOAL.md` and from the sealed research lineage (see `docs/RESEARCH-REFERENCE.md` for which experiment established which). Syntax in examples is illustrative; the contract is not.

A change that would violate one of these is a **stop-and-surface** decision under `AGENTS.md` §3, never a local call.

---

## C1. Five lifetimes stay distinct

```text
persistent runtime state        listener, scheduler, engine, observability, resource managers
immutable application definition compiled code, workload metadata, schemas, reusable engine state
execution world                 per-work mutable state, auth/tenant context, leases
resource lease                  temporary ownership of a persistent resource
external operation              work that may outlive the world that started it
```

- No type, struct, or module may hold two of these under one owner "for convenience".
- The application definition is immutable after build/load and reusable across many worlds.
- Definition-level work (linking, export resolution, schema compilation) happens at definition lifetime, not per world.

**Test shape:** one definition creates N worlds; mutation in world A is invisible in world B; destroying all worlds leaves the definition and runtime infrastructure intact.

## C2. Workload kind determines default lifetime

| Family | Kinds | World lifetime |
|---|---|---|
| Finite | HTTP request, task, cron invocation, queue message, command, migration, seeder | one invocation |
| Connection-bound | WebSocket, streaming HTTP response | the connection/stream |
| Persistent | service | the service |

- Developers do not annotate values as ephemeral/persistent; the workload gives the default.
- Persistent workloads are first-class, not a failure of the model. `headers sent != work complete` for streams.
- Connection-local state ends with the connection. It is never silently promoted to process-global state.

## C3. No implicit detached work

Asynchronous work is either **owned by the current world** or **explicitly transferred to another lifetime**.

```text
invoke   -> current world owns and awaits the child
dispatch -> ownership transferred to a task runtime; parent may finish
```

A finite world that reaches its terminal state with live asynchronous work (timers, unawaited promises, intervals) is a lifecycle error. The runtime must detect it and produce a diagnostic that names the fix (`task()` / `cron()` / `service()`). This is runtime behavior, not linting.

**Test shape:** handler schedules `setTimeout` and returns; runtime reports/cancels the orphan and the world still ends; the timer never fires in a dead world.

## C4. Death is not cleanup; ownership is cleanup

World destruction proves nothing about the external operations the world started or the resources it leased.

- Every external operation has an owner independent of the world.
- Cancelling a world **logically** cancels its operations (completions are dropped) but does not by itself terminate the physical operation.
- A completion arriving for a dead or wrong-incarnation world is rejected at one gate, identity-first, and never enters application code.
- Ownership state is bounded by live work: released records do not accumulate for the process lifetime.

**Test shape:** abort a world mid-operation; the operation's owner still reaches a terminal state; the late completion is rejected; ownership counters return to baseline.

## C5. Resource reuse requires terminal proof (PostgreSQL contract)

| Path | Rule |
|---|---|
| Normal completion | query reached terminal state → connection reusable → return to pool |
| Ordinary SQL error | protocol state known and terminal (SQLSTATE) → return to pool |
| Cooperative cancel / timeout | send CancelRequest → **await the original operation's terminal outcome** (57014) → only then reusable |
| Hard abandonment while in flight | terminal state unknown → **quarantine → remove from pool → replace**; never reuse |

- Backend identity and prepared statements are properties of a physical connection, established once per connection, not per world.
- The guest never sees SQL text, connections, credentials, cancel tokens, or host pointers; it sees an opaque capability with bounded, copied payloads.

**Test shape:** each of the four paths, plus a "recovery request succeeds after quarantine" test. No connection-state leak across worlds.

## C6. Fail before the world exists

Requests that cannot become application work must not create a world:

```text
no matching route
malformed transport input
body/params/query/header schema failure
missing required header
authentication rejected before application execution
```

World creation is aligned with meaningful application work. The HTTP pipeline order is fixed: route → decode → boundary validation → auth → admit → create world → handler → encode/enforce response contract → commit → settle ownership → world ends.

## C7. Boundary validation is the runtime's; business validation is the application's

Runtime: shape, required fields, UUID/email/enum, declared numeric limits, transport decoding, message contracts.
Application: balance, state transitions, business permissions, inventory rules, domain invariants.

Do not add domain rules to the runtime.

## C8. One source of truth: the ApplicationDefinition

`usai inspect`, `usai graph`, OpenAPI generation, generated clients, tests, and the dev server all read the same ApplicationDefinition the runtime executes. There is no parallel annotation, decorator, or config model that can drift.

- The application graph is explicit and deterministic (explicit `defineApp`/`defineModule` roots; no topology discovered through filesystem side effects).
- Raw/low-level endpoints are represented explicitly as partially or fully opaque in generated metadata.

## C9. Configuration maps structure; it does not invent semantics

- `usai.config.ts` describes entry, discovery (migrations/seeders), build, and output policy.
- Deployment settings (port, concurrency, CPU/memory limits, secrets) come from runtime/environment/CLI/orchestrator, not the app definition.
- Configuration is declarative and deterministic: no network calls, database queries, wall-clock reads, shell commands, or external mutation during definition.
- Precedence: explicit module/project declaration → root project config → built-in default. `usai config` shows the effective value and its source.
- Missing required environment fails activation, not the first request.

## C10. Persistent application state is an explicit resource

No magic `persistent(() => new Map())`. Persistence classes are distinct resource contracts (`postgres(...)`, `cache.local(...)`, …) with declared ownership, sharing, durability, and restart semantics. A new persistence primitive with unclear concurrency/durability semantics is a stop-and-surface decision.

## C11. Errors are contracts

- Known application errors map to stable transport responses and may be declared on endpoints.
- Unexpected failures are sanitized by default; full diagnostic context goes to logs/traces, never to the client in production.
- Lifecycle diagnostics teach the model (name the violated lifetime and the correct primitive).

## C12. Observability derives from runtime truth and is cheap when off

- Records are keyed by application revision, workload, world identity, request/message/task identity, leases, external operations, cancellation, abandonment, and commit rights.
- Disabled tracing must not allocate, format, or lock on the hot path. "Disabled but still paying" is a defect.

## C13. Implementation costs that must not be re-paid

Each of these was measured in research as a cost the semantics never required:

```text
process-lifetime history structures scanned per request
unbounded retained bookkeeping for released/dead work
application-definition objects rebuilt per world
linker/import/export metadata rebuilt per request
eager formatting/allocation for disabled observability
eager resolution of all exports when lazy resolution suffices
rewriting whole kept-resident memory when pagemap-aware reset can avoid it
```

Preserve: bounded live ownership state, definition-lifetime reuse, pooling + copy-on-write where the engine supports it, lazy resolution, pagemap-aware reset, explicit capability ownership. These are lessons, not sacred code — a cleaner mechanism with the same semantics is preferred.

---

## Contracts added by ADR (v0)

These were closed by reasoning rather than by research evidence. They bind like C1–C13 but carry an ADR that can supersede them.

## C14. The runtime loads a built definition; it never discovers topology by executing bootstrap code — ADR-0009

Declarations are evaluated/transformed in a controlled build phase into a static ApplicationDefinition. Runtime filesystem scanning, `process.env`-dependent topology, and registering workloads from handlers are forbidden.

## C15. Independent lifetime does not imply durable existence — ADR-0010

`dispatch` transfers ownership, not durability. A locally dispatched task may be lost on crash; durability is a separately named substrate with a stated guarantee.

## C16. Failure is terminal by default; retry is explicit and substrate-named — ADR-0014

One trigger → one invocation. Retry exists only where declared and where the substrate states its delivery guarantee.

## C17. Resources are reused across revisions only on identity match — ADR-0011

`kind + logical name + configuration fingerprint + compatibility version`. Never by name alone.

## C18. Persistent state has no generic primitive — ADR-0007

`cache.local`, `postgres`, `ctx.state`, or service-local state. No `persistent(() => …)`.
