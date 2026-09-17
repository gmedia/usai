# Threat Model (v0)

> What Usai promises about safety, what it explicitly does not, and what an operator must provide. Baseline from ADR-0008; revised as D13 evidence accumulates.

## Trust boundaries

```text
┌──────────────────────────────────────────────────────────────┐
│ Operator's host                                              │
│  ┌────────────────────────────────────────────────────────┐  │
│  │ Usai runtime process              ONE TRUST DOMAIN     │  │
│  │  application code (all revisions) · resources · tasks  │  │
│  └────────────────────────────────────────────────────────┘  │
│        ▲ HTTP / WebSocket (untrusted)     ▲ PostgreSQL (trusted network)
└──────────────────────────────────────────────────────────────┘
```

- **Untrusted:** every byte that arrives over HTTP/WebSocket; every message in the queue table (it may have been inserted by anyone with database access); environment variables are trusted but validated for shape.
- **Trusted:** application code, the build artifact, the database, the operator's environment, the host.
- **Not a boundary:** execution worlds. Fresh-world isolation is a *correctness* property (no state leaks between units of work). It is **not** a sandbox against hostile application code. Do not run code you would not run in a plain Node process.

## What the runtime enforces

| Threat | Control | Where |
|---|---|---|
| Oversized request bodies | `HttpConfig.max_body_bytes` (1 MiB default) → 413 before any world | `http/pipeline.rs` |
| Malformed / invalid input reaching application code | JSON Schema validation before the world exists (C6); in-world provider validation otherwise | `http/pipeline.rs`, SDK |
| Slow / hung handlers | per-invocation deadline (finite work), CPU-slice watchdog for runaway synchronous code | `world.rs` |
| Memory exhaustion by one world | per-world heap limit (`QuickJsConfig.memory_limit`, 64 MiB default) and stack limit; the world faults, the runtime continues | `engine/quickjs.rs`, `hardening.rs` test |
| Unbounded concurrency | hierarchical budgets: runtime → application → workload → resource; refusal is immediate (503), never a queue inside a world | `admission.rs` |
| Leaked async work extending a request's lifetime | detached-work detection and cancellation at the finite world's terminal state (C3) | `world.rs` |
| Stale completions reaching a new world | identity-first routing gate; ledger deliverability | `world.rs`, `ownership.rs` |
| Silent WebSocket clients holding a world | idle timeout (`HttpConfig.socket_idle_timeout`, 300 s default) closes with 1008 and runs the close handler | `http/socket.rs` |
| Poisoned pooled database connections | reuse only after terminal proof; quarantine on ambiguity or connection-loss SQLSTATEs; session state reset on checkout | `resource/postgres.rs` |
| Secrets in artifacts / manifests | resources reference env *names*; values resolved at activation; fingerprints hash secrets | `resource/mod.rs`, SDK `postgres()` |
| Internal detail leaking in error responses | unexpected errors sanitized to `internal`; stacks only in logs; `expose_diagnostics` is development-only | `http/pipeline.rs` |
| Configuration doing I/O at definition time | `usai.config.ts` and the app module are evaluated in a capability-less world (all host operations refused) | `engine/mod.rs` `RefusingBindings` |
| Stale artifact served silently | CLI commands always rebuild unless `--artifact` is explicit; artifact code hash must match the manifest | `crates/usai-cli`, `definition.rs` |

## What the runtime does not provide (v0)

- **No TLS.** Run behind a TLS-terminating proxy or on a private network. PostgreSQL connections are `NoTls`.
- **No authentication of `/_usai/*` surfaces.** Status, metrics, docs are served only when enabled (`--status`, `usai dev`); enable them only on trusted networks.
- **No isolation between applications or tenants.** One runtime = one trust domain. Multi-application hosting needs a new ADR and threat model before untrusted co-tenancy.
- **No CPU accounting beyond the synchronous slice.** A handler that awaits in a tight loop is bounded by its deadline, not by CPU share.
- **No durable tasks.** A crash loses locally dispatched tasks (ADR-0010); the queue substrate is durable because PostgreSQL is.
- **No rate limiting per client.** Budgets are per workload/resource, not per caller.
- **No protection against a malicious build step.** `usai build` runs `node` and esbuild from the project's `node_modules`; the supply chain is the project's.

## Operator checklist

1. Terminate TLS in front of Usai; put PostgreSQL on a private network.
2. Do not pass `--status` on public listeners; scrape metrics from a private interface.
3. Set `DATABASE_URL` and other declared env in the deployment environment; activation fails loudly if they are missing or malformed.
4. Size `RuntimeConfig.max_worlds` and pool sizes to the host; watch `usai_world_budget` and `usai_resource{metric="quarantined"}`.
5. Run `usai db migrate` as a deploy step, never at startup.
6. Treat lifecycle `detached_work` warnings as application bugs.

## Open

- Per-world CPU time accounting and fairness (D13 follow-up).
- A hardened multi-tenant mode is a separate, evidence-backed decision (ADR-0008).
