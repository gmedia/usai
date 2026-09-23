# Threat-model verification (2026-09-23)

`docs/THREAT-MODEL.md` is a list of promises. This is the run that checked
them from outside the process, one probe per claim, on the tree at
`20e9477` (0.0.8 unreleased, debug build, this dev box).

**Result: 35 probes, 35 passed, 0 failed.** The two findings of the day are
not runtime bugs — they are bugs in the *instrument*, found by distrusting a
clean first result (below).

Run it yourself: `bash scripts/qualification/threat/run.sh` (about 90
seconds; `USAI=` to point at another binary). The fixture is
`scripts/qualification/threat/app` — one route per claim that can only be
checked by pushing against a running server. Nothing in it is an attack:
every probe asserts the outcome the document already promises, so a
deviation is a finding against the runtime *or* against the document.

## What was checked

| Claim (`THREAT-MODEL.md`) | Probe | Result |
|---|---|---|
| Oversized bodies → 413 before any world | 2 MB body on a raw route; the `worlds_created` gauge before and after | 413, gauge unmoved |
| Invalid input never reaches application code (C6) | a body failing two constraints | 400 `validation_failed`, `details.slot: "body"`, gauge unmoved |
| Slow handlers are bounded | a handler awaiting 30 s behind a 300 ms deadline | 504 `deadline_exceeded`, under a second |
| Runaway synchronous code is bounded | a busy loop of 30 s behind a 300 ms deadline | 504, under a second — the watchdog, not the loop |
| One world cannot exhaust the process | a handler allocating 4 GiB in 1 MiB blocks | 500 (sanitized), the next request 200 |
| No internal detail in error responses | a handler throwing a message containing a secret | `{"error":{"code":"internal"}}`, no secret, no stack |
| Logs carry the application's words, not the client's | a canary in body, query, header **and** cookie | 0 log lines contain it |
| The status token never appears in a log line | grep the run's whole log | absent |
| Secrets are not in the artifact | grep the manifest for the value, then for the name | value absent, name present |
| Operator surfaces are protected | status/metrics with, without and with a wrong token | 200 / 401 / 401 |
| The probes stay open | `/_usai/live`, `/_usai/ready` without a token | 200, 200 |
| A switched-off surface is not served | `USAI_SURFACES_OFF=metrics,docs` | metrics 404, status still 200 |
| Leaked async work does not extend a request | a handler returning with a 5 s timer live | 200 immediately |
| An undeclared hand-off is reported | dispatch without `dispatches(...)` | one WARN naming the fix |
| The control surface refuses a public bind without a token | `--control 0.0.0.0:…`, no `USAI_CONTROL_TOKEN` | refused at start |
| A tampered artifact is not served silently | one line appended to `app.js` | refused (code hash) |
| A signature can be required | signed artifact under `--require-signature` | serves |
| A tampered *signed* artifact is refused | one line appended after signing | refused |
| An artifact signed by another key is refused | a second `keygen`'s public key | refused |

## Two bugs in the instrument, not in the runtime

A suite that passes everything on its first run has not been tested yet; it
has only been written. Two probes were passing without proving anything:

1. **The signature probe was vacuous.** It read the public key from
   `$OUT/key.pub` — a file `usai keygen` never writes, because it *prints*
   the key. `--require-signature ""` was then refused for the wrong reason
   and the probe matched on the word "refus". It now parses the key from
   keygen's output, and checks three things instead of one: a correctly
   signed artifact **runs**, a tampered one is refused, and one signed by a
   different key is refused.
2. **"No world was created" proved nothing.** It compared the gauge before
   and after a rejected request while the gauge was still 0 — it would have
   passed against a runtime that never creates worlds at all. It now proves
   the counter moves on a served request first (1 → 2) and only then that a
   rejection leaves it alone.

## What this run does not cover

- **The WebSocket idle timeout** (1008 after `socket_idle_timeout`): covered
  by the connection campaign (`2026-09-18-p5-p6-qualification.md`,
  `conn-churn`), not re-probed here — it needs a WebSocket client and 300 s
  of wall clock, or the environment override.
- **Admission refusal under load** (503 `capacity_exhausted`, hierarchical
  budgets): covered by `crates/usai-runtime/tests/hardening.rs`
  (`budget_exhaustion_refuses_promptly_and_recovers`) and the P5/P6
  campaigns.
- **Stale completions reaching a new world** (the routing gate, C7): a
  property of the ledger, covered by the lifecycle suite; it cannot be
  reached from outside the process.
- **Poisoned pooled connections**: `crates/usai-runtime/tests/postgres.rs`
  (quarantine, terminal proof, and — new today — the refusal of raw
  transaction control).
- **Everything the document says it does *not* provide** (no TLS, no
  per-caller rate limit, no tenant isolation, no durable local dispatch, no
  CORS): those are not claims to verify, they are limits to respect. They
  are unchanged.
- The memory probe deserves one note: the guest's own heap limit caught the
  allocation (`InternalError: out of memory`) before the pooling allocator's
  linear-memory bound did, so the world ended as a handler error rather than
  a trap. Both outcomes are the documented one — the world dies, the runtime
  keeps serving — but the bound that fired is the inner one.

## What to do with this

Run it before every release, beside `make check`. It is cheap, it needs no
database and no VM, and it is the only thing in the tree that checks the
threat model rather than the runtime's own unit-level behaviour.
