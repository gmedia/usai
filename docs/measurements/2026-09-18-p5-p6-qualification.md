# P5 operational and P6 reliability qualification (2026-09-18)

Deployment: `scripts/qualification/p5/` on the research VM (16 × Xeon
E5-2680 v4, 32 GB, Docker 29.7): Caddy 2 (:8080) → `usai run --artifact
--status --control` in the runtime image built from the checkout (512 MB
memory limit, read-only, tmpfs `/tmp`) → PostgreSQL 18. Application:
`examples/invoicing` (real queries per request, sessions, transactions).
Load: `loadgen.mjs`, 8 closed-loop clients doing list → get → create,
≈1 000 req/s, p50 ≈ 6 ms through the proxy. Evidence files (per-second
JSON lines, status before/after, app log for the window) under
`scripts/qualification/p5/out/` and `p6/out/` on the VM; the numbers below
are copied from the runner's summaries.

Engineering numbers on one machine, not canonical evidence.

## P5 — deliberate failures under load (40 s windows, action at t≈8 s)

| Scenario | Impact | Recovery | Notes |
|---|---|---|---|
| baseline | 45 382 ok, 0 errors, p99 ≤ 30 ms | — | |
| PostgreSQL `kill -9` + start | 9 129 × **503** over 8 s, 0 × 5xx | first 200 within 1 s of `pg_isready`; 12 s total = PostgreSQL's restart | refusals fast: ~1 350/s at p99 < 10 ms; nothing queued |
| PostgreSQL `restart` | 567 × 503 in 1 s | 1 s | |
| network partition 10 s | 155 × 503 in 1 s after reconnect, 8 client errors | 11 s | dead pooled connections quarantined on first use, then replaced |
| SIGTERM → replacement | 2 s of proxy 502 (767) | 3 s | log: `SIGTERM received` → `shutting down: draining` → `drained; ownership returned to baseline`, exit 0; the gap is the single-replica window |
| SIGKILL → replacement | 3 s of proxy 502 (1 124) | 2 s | in-flight lost by design; DB state consistent |
| bad deployment (manifest format 99) | **0 impact** | — | control answers 422 `invalid_artifact` with the compatibility message; no revision created |
| rollback (activate rev2, re-activate rev1) | **0 failed requests** | — | `revision active` / `revision draining` pairs in the log; 6 s including the drain wait |
| traffic spike (+64 clients, 15 s) | 0 errors; p99 57 ms | — | no capacity refusals at 256 worlds |
| memory limit 96 MB | restart loop (OOM every ~3 s) until the limit was raised | 18 s | `dmesg`: `Memory cgroup out of memory: Killed process … (usai)`; see runbook |
| `/tmp` filled (tmpfs) | 2 s blip | — | tmpfs counts as memory; a real disk-full surfaces through PostgreSQL 53xxx → 503 |
| invalid config (`DATABASE_URL=`) | replacement never listens | previous keeps serving | `error: missing required environment: DATABASE_URL … usai run reads the process environment only …`, exit 1 |

Findings that changed the code (all merged): unavailable dependencies
answered 500 → now **503** (`pool_error`, `connection_closed`, PostgreSQL
class 08/53/57P0x, HTTP client connect/timeout); migrations did not ship
with the artifact → `usai build` copies them, `usai db migrate --artifact`;
held revisions were unbounded → `max_revisions` (8) with 409
`too_many_revisions`; control ids accepted bare or `rev`-prefixed;
replaced revisions now retire themselves once settled.

## P6 — reliability campaigns

(filled in from the campaign log; see the section below)
