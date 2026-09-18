# ADR-0017: Outbound HTTP is a declared resource, crypto is a bounded subset, a transaction is one owned operation

**Status:** accepted (v0)
**Date:** 2026-09-18
**Closes:** the three "P4 decisions" left open by ADR-0013 (`fetch` "likely yes", `crypto` "host-provided") and the known gap "no transactions across operations"

## Context

The first outside developer to build an API on Usai (fresh-eyes run, 0.0.4)
ended with: *"the single most important thing to fix is outbound HTTP with the
same ownership rules as tasks"*, and *"you cannot generate an ID in a
handler"*. A real application (roadmap P4) calls payment providers, sends
webhooks, hashes passwords, mints tokens, and needs several statements to
commit or fail together. Each of these has a lifetime question ADR-0013 says
must be answered before the API exists: who owns it, what happens at world
end, what happens on cancel.

## Decision

**Outbound HTTP is a resource, `httpClient(name, { baseUrl | baseUrlEnv, timeoutMs, maxConcurrent, headers, bearerTokenEnv })`, not a global `fetch`.**
The runtime owns the client (connection pool, TLS roots, redirect policy, user
agent) for its lifetime; each `ctx.resources.<name>.fetch(url, init)` is one
operation owned by the world. Cancelling or ending the world drops the
request — terminal for the client side, so nothing is quarantined. A declared
`baseUrl` pins the destination: another origin is refused inside the world
(`origin_refused`). The in-flight bound refuses (`resource_exhausted`, 503)
rather than queues. The destination is visible in `usai graph`, `inspect`,
`/_usai/status` and the API docs like any other resource. `globalThis.fetch`
exists only to reject with `fetch_not_available` and the fix.

**`crypto` is a WebCrypto subset:** `getRandomValues`, `randomUUID`,
`subtle.digest` (SHA-256/384/512), `subtle.importKey/sign/verify` (HMAC).
Randomness is 32 bytes of host entropy installed per world with the invocation
(`__usai.seed`), expanded with SHA-256 in counter mode — never seeded into the
image, whose state is copied into every world. Digests and HMAC are pure
JavaScript in the bridge. Password hashing is a **host operation**
(`password.hash/verify`, Argon2id on the blocking pool): deliberately
expensive work never runs on a world's thread.

**A PostgreSQL transaction is one owned operation.** `db.transaction(fn)`
leases a connection into a holder task (`begin`), the callback's statements
route to it by lease, and `commit`/`rollback` end it. While open, the guest
holds a synthetic pending entry, so a finite world that returns with the
transaction open is a C3 violation with a teaching diagnostic; the holder
watches the world's cancellation and issues `ROLLBACK` on its behalf. The
connection returns to the pool only when that rollback reached a terminal
state; otherwise quarantine, exactly as for a single statement (C5).

## Consequences

- Easier: egress, secrets and transactions all follow the model already used
  for statements and tasks; nothing new to learn about lifetimes.
- Harder: libraries that call `fetch` internally do not work (they were never
  going to under ADR-0013); the crypto subset excludes RSA/ECDSA/AES for now.
- Forbidden: a global `fetch`; randomness seeded before the image snapshot;
  a transaction API that hands the connection to the handler without a holder.

## Alternatives considered

- **Global `fetch` owned implicitly by the world.** Rejected: hidden egress —
  the graph and docs would not show what an application talks to, and there
  is no place for a per-destination bound or credential.
- **Host op per `crypto` call.** Rejected for digests/HMAC/random: a host
  round trip per call for pure computation re-pays a cost the image was built
  to avoid (C13); accepted for Argon2id because the cost *is* the point.
- **Transactions as "hold the connection in the world".** Rejected: the
  world cannot own a physical connection (it dies without notice); the holder
  task is the owner, the world only holds a lease id.
