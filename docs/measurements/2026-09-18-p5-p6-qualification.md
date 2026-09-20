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

## P6 — reliability campaigns (`scripts/qualification/p6/run.sh`)

| Campaign | Result |
|---|---|
| revision churn, **1 000 replacements** (install + activate through the control surface, previous retires itself) under 8-client load | 262 s for the replacements; over the whole 2 019 s window **2 039 500 requests, 0 errors, 0 × 503, 0 bad seconds**, p99 median 29 ms; RSS 143 MiB after, `compiledImagesLive` 2, one revision held |
| PostgreSQL flap ×10 (kill −9, 2 s down, 8 s up) | 87 361 ok, **0 × 5xx**, 4 121 × 503 during the outages, 19 connections quarantined in total, pool back to 8 with 5 available |
| runtime restart loop ×10 (SIGTERM → drain → replacement) | mean 1 s per cycle; 10 `drained; ownership returned to baseline` lines; the single replica's 502 window ≈ 1.7 s per cycle (8 645 over 10) |
| overload: 200 clients against `--max-worlds 48` | **103 258 × 503 `capacity_exhausted`, 0 × 5xx**, admitted requests p99 median 140 ms (max 472 ms); `usai_http_rejections_total{reason="capacity"}` 103 899 |
| idle 60 s → 32-client burst | first second p50 25 ms / p99 214 ms, second p50 18 ms; 0 errors |
| dead letter: endpoint always 500 | 5 attempts (500 ms exponential: 0.5, 1, 2, 4 s), `retried` 4, `dead` 1, five `webhook_deliveries` rows, `usai_queue.state = dead` with the error |
| soaks (1 h, 24 h; 72 h running) and the connection campaign | below |

What the churn campaign found before it passed: RSS grew ~2 MB per
replacement until the container's 512 MB limit killed the process after
~270 replacements (`dmesg: Memory cgroup out of memory: Killed process …
(usai)`). Not a leak — `compiledImagesLive` stayed at 2 — but glibc malloc
arenas: one per worker thread, each retaining a freed module-sized chunk
the other arenas cannot reuse. `M_ARENA_MAX=2` at startup bounds it (300
local replacements: 290 MB → 110 MB plateau). Also found: Wasmtime prefers
cold slots for a new module and keeps 100 warm unused ones
(`max_unused_warm_slots`), so churn touched ever more keep-resident pages;
set to 0, slots used = peak concurrency.

## Soaks (appended 2026-09-20)

Same deployment, same 8-client load through Caddy (`loadgen.mjs`, list →
get → create on `examples/invoicing`), a status sample every 60 s.

| Soak | Window | Result |
|---|---|---|
| 1 h (different setup: `usai bench -c 16` against the runtime directly, hello, no proxy — `2026-09-18-execution-path-attribution.md` §8) | 2026-09-18 | 49 M requests at 13.6k req/s, 0 errors, RSS +0.9 % |
| **24 h** | 2026-09-18 22:18 → 2026-09-19 22:19 UTC (86 374 s of load; 8 closed-loop clients ≈ 405 req/s of list → get → create through the proxy) | **34 978 737 ok, 0 × 4xx, 0 × 5xx, 0 × 503**, 9 client errors in **5 bad seconds** (below); p99 median 273.9 ms; RSS 54.8 → max 58.1 → 56.2 MiB at the end (1 416 samples); 35 007 080 worlds created, `liveWorlds` max 8 and 0 at the end, `detachedWorkDetected` 0, completions dropped late / rejected stale 0; PostgreSQL pool: 89.2 M operations, 3.50 M transactions, 9 cancelled, 6 rolled back for a dying world, **0 quarantined**, 8/8 available at the end |
| 72 h | started 2026-09-19 22:28 UTC → ends 2026-09-22 22:28 UTC | running; appended when it ends |

**The 5 bad seconds** (t = 28 741–28 745 s and 40 325–40 326 s, p99 ≈ 10 001 ms
= the client's own 10 s timeout): both episodes are whole-second gaps in
which the load client itself completed 0–34 requests instead of ≈400, the
application log has no WARN or ERROR line, and `journalctl -u docker` on the
host has, at the same seconds, `dockerd … healthcheck failed fatally` at
06:17:45–50 UTC (a PHP comparator's images being pulled and built for the
P8 report) and `image pulled ubuntu:26.04` at 09:30:46 UTC (the P8E fleet's
base image). The VM stalled for the whole host while Docker extracted image
layers; the runtime, the client and the proxy all stalled with it. Reported
as what it is — a host stall caused by the operator's own activity, not a
runtime defect — and the reason the 72 h soak runs with no image pulls on
the host.

## Connection campaign (`p6/run.sh conn-churn`, 2026-09-19 22:2x UTC)

The deployment-level WebSocket/SSE evidence `SUPPORTED.md` claims, on the
same deployment with `--max-worlds 48` and `USAI_SOCKET_IDLE_TIMEOUT=20`:

| Scenario | Result |
|---|---|
| 500 cycles × (SSE two events + WS hello/ask/answer), 16 at a time | 6.9 s; SSE 500 ok (p50 207 ms to the second event), WS 500 ok (p50 6.3 ms), 0 errors; live worlds back to 0 in 0 s |
| 40 connections held through a **revision replacement** (control surface, t=10 s) | previous revision drained in 0 s; all 40 closed at the drain bound — 20 × WS `1012 server draining`, 20 × SSE ended; 0 failed probes; live worlds 0 |
| 40 held through an **app restart** (SIGTERM → drain → start) | app exited after 0 s, healthy again after 2 s; 20 × `1012`, 20 × SSE ended; 0 failed probes |
| 40 held through a **proxy restart** (Caddy) | proxy back after 11 s; the 40 client-side connections died with the proxy (20 × WS `1006`, 20 × SSE terminated) and 20 reconnects failed while it was down — the proxy's behaviour, not the runtime's; live worlds 0 afterwards |
| **abrupt client death** (40 connections, client `kill -9`, no close frames) | the runtime does not learn of a dead peer that sent no FIN/RST: **live worlds still 40 after 30 s**, and SSE probes got 503 (48 slots, 40 held) from t≈37 s. The connection-bound worlds ended at the idle timeout (20 s of silence → `1008 idle timeout`, then the fleet returned to 0). **Sizing rule**: `--max-worlds` must exceed the number of connections you are prepared to hold through `USAI_SOCKET_IDLE_TIMEOUT` plus the request concurrency, and the idle timeout is the bound on how long a vanished client costs a slot |
| a client that never reads: 10 SSE connections unread for 30 s | RSS 89.7 → 96.7 MiB (+0.7 MiB per unread stream: the bounded send buffer), live worlds 0 afterwards |
| idle socket, nothing sent | closed by the server with `1008 idle timeout` after 20 s, as documented |

## Two replicas (`p6/replicas.sh`, 2026-09-20 02:04–02:40 UTC)

The topology `SUPPORTED.md` states, campaigned: Caddy (round robin, active
health check on `/_usai/ready` every 1 s, a refused connection retried on the
other upstream for up to 5 s) → `app` + `app2` (`app2` with `USAI_NO_CRON=1`,
both `--max-worlds 48 --drain-timeout 10`) → one PostgreSQL, one compose
project (`scripts/qualification/p5/compose.replicas.yaml`,
`Caddyfile.replicas`) pinned to cpus 12–15 beside the running 72 h soak
(disclosed co-tenant, cpus 0–11). Application: `examples/invoicing`. Load:
8 closed-loop clients through the proxy. The first run used the soak's image
(0.0.5 + fixes at `615dc4e`); the final run a box built from `ubuntu:26.04`
with the host-built binary at `d3f3c88` and the same artifact (the runtime
image is bookworm and the fixes needed a rebuild the soak forbade).

| Scenario | Pass rule | Result |
|---|---|---|
| HTTP sharing, 60 s | 0 × 5xx, both replicas served | 37 988 ok, 0 errors; worlds created **19 005 / 19 005** |
| Queue sharing: 1 000 invoices issued → 1 000 webhook deliveries | delivered exactly 1 000 times in total, both replicas consumed, 0 dead, 0 pending | **1 000 deliveries (sink saw 1 000)**, done by app 497 / app2 503, dead 0 |
| Two `migrate` jobs at once on a fresh database | both exit 0, three files applied once | exit 0/0, `usai_migrations` 3 rows |
| Cron on one replica | `scheduler.cron` true on app, false on app2 | as expected; `usai_scheduler{kind="cron"}` 1/0. Through the proxy, `/_usai/status` alternates between the two answers — status behind a balancer is one replica's (runbook) |
| Rolling restart under load (app2 at t=10, app at t=35; 20 SSE/WS held) | 0 × 502, held connections on the restarted replica closed `1012`/ended, none `1006` | **first run: 1 × 502** (Caddy `EOF`: a request written onto an idle keep-alive connection at the instant the listener closed) → fixed with the drain grace (below) → **final run: 40 536 ok, 0 × 5xx, 0 errors**, ready again 3–4 s after each restart, closes 10 × `1012` + 10 × SSE ended, probes never failed |
| One replica `kill -9` under load with deliveries mid-flight (endpoint 2 s per call) | ≤ 16 × 5xx (requests in flight on the dead replica), every message delivered, nothing left pending | **first run: 4 messages stuck `processing` forever** (claimed by the dead replica's four workers; nothing reclaimed them) → fixed (below) → **final run: 0 × 5xx**, 4 messages reclaimed 25 s after the kill and delivered (300 deliveries, sink saw 304 — at-least-once, as declared), nothing pending, the replica back in 1 s (`restart: unless-stopped`) |

Two fixes came out of it, both in `d12d44f`:

- **`usai run --drain-grace <s>`** (`USAI_DRAIN_GRACE`, default 2). On
  SIGTERM the listener stays open while `/_usai/ready` answers 503
  `draining` and every response carries `Connection: close`; the listener
  closes and the drain starts after the grace. A balancer with an active
  health check learns, and stops reusing idle connections, *before* they are
  refused.
- **Lost-consumer reclaim.** One sweeper per topic returns a message whose
  claim is older than the message deadline + 10 s to `ready` when the
  declared retry policy has attempts left (the claim had already counted
  one), or dead-letters it with `consumer lost: claimed by <replica:topic:
  worker> at <time>, never completed`. `/_usai/status` counts them
  (`queue.reclaimed`).

Two things the campaign taught that are not bugs: `docker kill` counts as a
manual stop and the restart policy does not bring the container back (the
campaign kills the process from the host); and a message redelivered after a
reclaim is a duplicate the consumer must tolerate — at-least-once was always
the contract, this is one of the ways it shows.

## Not done here

Three or more replicas, a replica set across hosts (the queue and the
migration lock are PostgreSQL-side and do not care; the proxy configuration
does), and the 72 h soak (running).
