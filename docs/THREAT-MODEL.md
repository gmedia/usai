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
| Oversized request bodies | the request body bound (1 MiB default; `USAI_MAX_BODY_BYTES`) → 413 before any world | `http/pipeline.rs` |
| Malformed / invalid input reaching application code | JSON Schema validation before the world exists (C6); in-world provider validation otherwise | `http/pipeline.rs`, SDK |
| Slow / hung handlers | per-invocation deadline (finite work), CPU-slice watchdog for runaway synchronous code | `world.rs` |
| Memory exhaustion by one world | per-world linear-memory bound (`WasmConfig.max_memory_bytes`, 64 MiB default, enforced by the pooling allocator) and the guest's own heap limit; the world faults, the runtime continues. The reference engine has the same bound (`QuickJsConfig.memory_limit`) | `engine/wasm.rs`, `engine/quickjs.rs`, `hardening.rs` test |
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

- **No HTTP TLS.** Run behind a TLS-terminating proxy or on a private network. PostgreSQL connections use TLS when the URL says `sslmode=require` (or `prefer`, the default, when the server offers it) and always verify the server certificate (Mozilla roots + `tls.caFile` / `PGSSLROOTCERT`).
- **`/_usai/*` surfaces are operator information.** Status, metrics and the reference are served only when enabled (`--status`, `--status-addr`, `usai dev`). `USAI_STATUS_TOKEN` puts them behind a bearer token (constant-time compared; the probes `/_usai/live` and `/_usai/ready` stay open, the docs shell asks for the token); without a token, serve them only on a private listener; `USAI_SURFACES_OFF` removes the ones nobody consumes. What they reveal: RSS/PSS and fault counts, per-route request and error counts (a traffic map), the revision identity, resource fingerprints and pool sizes, the full OpenAPI profile (environment variable names, cron schedules, queue message schemas).
- **No isolation between applications or tenants.** One runtime = one trust domain. Multi-application hosting needs a new ADR and threat model before untrusted co-tenancy.
- **CPU is accounted, not scheduled.** Every world's guest CPU time is measured (thread CPU inside guest entries: `WorkResult.cpu`, the world trace's `cpu_us`, `usai_guest_cpu_seconds_total`) and one synchronous run is bounded by the CPU slice; there is no per-workload CPU share or fairness policy yet — a handler that awaits in a tight loop is bounded by its deadline.
- **No durable tasks.** A crash loses locally dispatched tasks (ADR-0010); the queue substrate is durable because PostgreSQL is.
- **No rate limiting per client.** Budgets are per workload/resource, not per caller.
- **Browsers: no CORS, no CSRF token, no cookie policy beyond the defaults.** The runtime does not answer preflights and never adds `Access-Control-*` headers (the proxy does, or the application declares `http.options` routes and `defineApp({ headers })`); it does not check `Origin`/`Sec-Fetch-Site`. What stands in for a CSRF defence is the cookie helper's default `SameSite=Lax; Secure; HttpOnly` plus the fact that contract routes accept only `application/json` bodies (a cross-site HTML form cannot reach a handler; a cross-site `fetch` with credentials needs the proxy's CORS consent). An application that sets `SameSite=None`, or that takes raw routes with cookie auth, owns its own CSRF check (GUIDE §4 → Cookies in the browser).
- **The artifact is trusted, including `cache/image.cwasm`.** The precompiled image is native code that `usai run` loads into the process when its digests match `image.json`; a party who can write the artifact directory can already replace `app.js` (which runs sandboxed with the application's full capabilities) and with `image.cwasm` could run native code. Deploy only artifacts you built, or require signatures (`usai build --sign` + `usai run --require-signature`, next section); `USAI_PRECOMPILED=0` disables loading the native image.
- **Logs carry what the application says, not what the client sent.** A 5xx line has the world, workload, request id, error code, the application's error message, contract-failure details and a source-mapped stack — never request bodies, headers, parameters or the principal (GUIDE §14). The status token and the control token never appear in a log line (the control surface logs `unauthorized` without the presented value).
- **Rotating a token.** `USAI_STATUS_TOKEN` and `USAI_CONTROL_TOKEN` are read at start: rotate by restarting with the new value (a rolling restart behind a proxy costs nothing, `runbooks/deploy-and-rollback.md`); a leaked control token means an attacker could install a revision until then — with `--require-signature` only an artifact you signed.
- **No protection against a malicious build step.** `usai build` runs `node` and esbuild from the project's `node_modules`; the supply chain is the project's.

## Artifact integrity

`cache/image.cwasm` is native code: whoever can write the artifact directory
can run code in the runtime process. `usai build --sign` + `usai run
--require-signature` (GUIDE §14) closes that: the runtime verifies an Ed25519
signature over every file before loading anything, and the control surface
verifies installs the same way. Without `--require-signature` the artifact
directory is trusted like the binary itself — protect it the same way.

## Container envelope

The runtime image (`sakaladev/usai:X.Y.Z`) runs as user `usai` (10001), ships
no Node or compiler (it is Debian slim, so a shell and coreutils exist —
not distroless), reads
configuration from the environment only, and serves a precompiled artifact
with `USAI_COMPILE_CACHE=0`, so `--read-only` (with a `tmpfs` at `/tmp`) is the
recommended way to run it. `docker stop` (SIGTERM) drains like Ctrl-C. The
`-dev` image is for building and developing and is not a production base.
Status/control surfaces stay on trusted networks in a container exactly as
on a host: prefer `--status-addr` on a private port over `--status` on the
application listener, and never publish the control port.

## Operator checklist

1. Terminate TLS in front of Usai; use `sslmode=require` to PostgreSQL (with the provider's root in `tls.caFile`) or keep it on a private network.
2. Do not pass `--status` on public listeners; scrape metrics from a private interface.
3. Set `DATABASE_URL` and other declared env in the deployment environment; activation fails loudly if they are missing or malformed.
4. Size `RuntimeConfig.max_worlds` and pool sizes to the host; watch `usai_world_budget`, `usai_worlds_live` and `usai_resource_quarantines_total`.
5. Run `usai db migrate` as a deploy step, never at startup.
6. Treat lifecycle `detached_work` warnings as application bugs.

## Open

- Per-workload CPU fairness / budgets on top of the accounting (D13 follow-up).
- A hardened multi-tenant mode is a separate, evidence-backed decision (ADR-0008).
