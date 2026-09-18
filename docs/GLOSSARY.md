# Glossary

Shared vocabulary for this repository. Production terms first; research-era terms at the end so agents can read the evidence without re-deriving them.

## Production terms

**Runtime** — the persistent host process: network listener, scheduler, engine/allocation infrastructure, observability, resource managers. Lives for the process. Owns application definitions and resource managers.

**ApplicationDefinition** — the immutable, inspectable description of one application revision: compiled/bundled handlers, workloads and their triggers/contracts/policies, resources, config requirements, migration/seeder metadata, and reusable definition-level engine state. Built once, reused across many worlds. The single source of truth for routing, tooling, docs, and tests.

**Revision** — one built ApplicationDefinition as deployed. Multiple revisions may coexist during activation/drain.

**Module** — an organizational and ownership unit that groups workloads, resources, migrations, and seeders. Not a process, lifetime, deployment unit, or security boundary.

**Workload** — a declared kind of application work with a natural lifetime: HTTP request, task, cron invocation, queue message, command, WebSocket connection, stream, service.

**Finite / connection-bound / persistent** — the three lifetime families of workloads (see `LIFECYCLE-CONTRACTS.md` C2).

**WorkWorld (execution world, world)** — the disposable execution context for one unit of work: mutable application globals, request/message/task state, auth/tenant context, temporary allocations, resource leases. Created after admission, ends at the workload's terminal state.

**ExecutionContext (`ctx`)** — the typed handle a handler receives inside a world: params/query/body/message, resources, tasks, signal, state (for connection-bound workloads).

**Resource** — state or capability whose lifetime is intentionally longer than one world: PostgreSQL pool, cache, secrets/config provider, storage, queue infrastructure. Declared explicitly with ownership semantics.

**Resource manager** — the runtime-owned component that owns a resource for the runtime/revision lifetime and hands out leases.

**Lease** — temporary ownership of a persistent resource by one world (e.g. one pooled connection). Lease lifetime is separate from world lifetime and from the external operation's lifetime.

**External operation** — work that may continue outside the world that initiated it (a SQL query in flight, an outbound request). Has its own owner and terminal state.

**Terminal state / terminal proof** — knowledge that an external operation has definitively ended (success, known SQLSTATE, confirmed cancellation). Only terminal proof makes a resource reusable.

**Quarantine** — the fail-closed decision for a resource whose operation ended ambiguously: remove from the pool, replace, never reuse.

**Invoke vs dispatch** — `invoke` keeps ownership in the current world and awaits the child; `dispatch` transfers ownership to a task runtime so the parent may finish.

**Detached work** — asynchronous work with no owner. Forbidden (C3).

**Boundary validation** — transport/structural validation done by the runtime before a world exists. Distinct from **business validation**, done by the application.

**Raw endpoint** — the explicit low-level HTTP escape hatch (`http.raw`). Runtime still owns lifetime/cancellation/ownership; schema validation and generated docs may be partial.

**Admission** — the runtime's decision to accept work and create a world (after routing/validation/auth, subject to capacity).

**Revision states** — `installed → active → draining → retired` (ADR-0006). Installed = validated, not serving; active = serving; draining = no new work, in-flight worlds settle; retired = gone. A failed activation never displaces the active revision.

**ResourceIdentity** — `kind + logical name + normalized configuration fingerprint + compatibility version` (ADR-0011). Two revisions share a resource manager only when identities match.

**Durable vs independent** — `dispatch` gives a task an independent lifetime, not durable existence (ADR-0010). Durability is a separately named substrate.

**Trust domain** — v0 runtime is one trust domain: all code in it is operator-trusted (ADR-0008). Fresh-world isolation is semantic, never described as a sandbox.

**Sakala** — a separate open-source deployment/orchestration project in the same ecosystem. Future first-class integration; never a runtime dependency.

## Research-era terms (you will meet these in the private research repository)

**EXP-NNN** — a preregistered experiment. `*-RESULT.md` is sealed and immutable. `*-FREEZE.json` binds the exact source/binaries for a one-shot attempt.

**Sealed** — a research record that may never be edited. Corrections are new documents.

**Canonical** — evidence produced under the freeze/one-shot protocol. Development/smoke runs are never canonical.

**R1 / R2 / R3** — successive host-implementation candidates in the HTTP+PG economics lineage. R3 (EXP-012B) is the one whose lessons this repo inherits. R1/R2/R3 terminology must not appear in production code or developer-facing docs.

**Incarnation** — research name for one world's identity; used to reject completions addressed to a dead or different world. Production equivalent: world identity.

**Logical vs physical operation** — logical = the guest-visible async op tracked per incarnation; physical = the host-side external operation (e.g. a query on a real connection) with its own ledger record. Logical cancellation never removes the physical record.

**Ledger** — the indexed, bounded record of live physical operations (R3 §4.1). The lesson: index by id and by owner, reclaim on release, retain history only when recording evidence.

**Phase S / B0 / C / E** — stages of EXP-012B: semantic oracles, continuity classes, 64-world concurrency prerequisite, economics sweep.

**Inherited envelope** — the preregistered pass/fail gates (p50 ratio, p99 ratio, absolute p50 delta, CPU ratio) against the Node/Fastify comparator.

**Image / application image** — the Wizer-pre-initialized Wasm module containing QuickJS with the application already evaluated. Production analogue: the compiled part of an ApplicationDefinition.

**Pooling + COW** — Wasmtime pooling allocator with `memory_init_cow`, giving cheap fresh worlds by copy-on-write from the image. Current physical representation; not part of the public model.

**PAGEMAP_SCAN** — Linux kernel facility Wasmtime uses to reset only dirtied pages of kept-resident memory (R3 §4.5).

**usai** — Indonesian: finished, over, done. The name is the contract: a thing ends when its work no longer needs it.
