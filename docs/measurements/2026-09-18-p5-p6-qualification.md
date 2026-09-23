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

## Soaks (appended 2026-09-20, the 72 h run 2026-09-23)

Same deployment, same 8-client load through Caddy (`loadgen.mjs`, list →
get → create on `examples/invoicing`), a status sample every 60 s.

| Soak | Window | Result |
|---|---|---|
| 1 h (different setup: `usai bench -c 16` against the runtime directly, hello, no proxy — `2026-09-18-execution-path-attribution.md` §8) | 2026-09-18 | 49 M requests at 13.6k req/s, 0 errors, RSS +0.9 % |
| **24 h** | 2026-09-18 22:18 → 2026-09-19 22:19 UTC (86 374 s of load; 8 closed-loop clients ≈ 405 req/s of list → get → create through the proxy) | **34 978 737 ok, 0 × 4xx, 0 × 5xx, 0 × 503**, 9 client errors in **5 bad seconds** (below); p99 median 273.9 ms; RSS 54.8 → max 58.1 → 56.2 MiB at the end (1 416 samples); 35 007 080 worlds created, `liveWorlds` max 8 and 0 at the end, `detachedWorkDetected` 0, completions dropped late / rejected stale 0; PostgreSQL pool: 89.2 M operations, 3.50 M transactions, 9 cancelled, 6 rolled back for a dying world, **0 quarantined**, 8/8 available at the end |
| **72 h** | 2026-09-19 22:28 → 2026-09-22 22:28 UTC (259 131 s of load, same 8 clients) | **73 627 701 ok, 0 × 5xx, 0 × 503, 2 × 4xx**, 36 client errors in **15 bad seconds** (two episodes, below); RSS 51.5 → max 54.9 → 53.5 MiB over 4 103 samples, fds 29 → 30, threads 19; **73 627 744 worlds created**, `detachedWorkDetected` 0, completions dropped late / rejected stale 0, `liveWorlds` 0 at the end; PostgreSQL pool **0 quarantined**, `waiting` 0 in every sample; the runtime logged no WARN or ERROR line in three days (the five WARNING lines in the container log are PostgreSQL notices, `there is no transaction in progress`, at the two error episodes) |

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

**The 72 h soak's throughput decays and its tail grows — and neither is the
runtime.** Over the three days the closed-loop rate fell 794 → 154 req/s and
the per-second p99 rose 84 → 810 ms, while the per-second p50 stayed flat at
4.6 → 5.3 ms:

| Window | avg req/s | p50 median | p99 median | p99 p95 |
|---|---|---|---|---|
| 0–6 h | 794 | 4.6 ms | 84 ms | 255 ms |
| 12–18 h | 349 | 5.1 ms | 289 ms | 467 ms |
| 24–30 h | 244 | 5.8 ms | 450 ms | 587 ms |
| 48–54 h | 181 | 5.4 ms | 666 ms | 819 ms |
| 66–72 h | 154 | 5.3 ms | 810 ms | 941 ms |

The runtime's own per-request cost is flat across the same window — guest
CPU 1.56 → 1.59 ms, process CPU 2.80 → 3.04 ms, SQL operations 2.52 → 2.59
per request, pool `waiting` 0 throughout, RSS and fds flat. What grew is the
**data the workload accumulated**: the mix is 10 % `POST /invoices`, so the
soak wrote ≈7.4 M invoices with their items into one table over three days
(the list is keyset-paginated on `invoices_page` and stays O(limit); the
inserts, the index growth, the WAL, the checkpoints and autovacuum do not).
The application spends more wall-clock waiting for PostgreSQL and less CPU
per second as it slows: a three-day soak of this shape is as much a
PostgreSQL soak as a runtime one. The runtime's invariants — memory,
descriptors, ownership, the pool, the error count — held for 73.6 M worlds.
Read as a runtime result: **no leak, no drift, no refusal, no quarantine in
73 627 744 worlds**; read as a deployment result: a single unpartitioned
table that takes 7 M inserts in three days needs the usual PostgreSQL care,
and the soak does not pretend otherwise.

Server-side latency over the whole soak (the runtime's own histogram, which
includes the PostgreSQL wait): 80.8 % of requests ≤ 5 ms, 89.9 % ≤ 10 ms,
96.6 % ≤ 250 ms, 98.9 % ≤ 500 ms, 99.97 % ≤ 1 s, 126 requests (0.0002 %)
above 5 s. Mean 26.6 ms — the mean is the tail, and the tail is the
database's write path late in the soak.

**The 15 bad seconds** (36 client errors, all with the client's own 10 s
timeout as the second's p99): two episodes, 2026-09-20 02:24–02:26 UTC (10
seconds, 24 errors) and 2026-09-21 06:17 UTC (5 seconds, 12 errors). In both
the client completed 0–64 requests in the second instead of ≈300, the
runtime served no 5xx and logged nothing, and PostgreSQL logged `WARNING:
there is no transaction in progress` at exactly those instants. Both fall
inside windows when the operator ran other work on the same host (the
two-replica campaign was 02:04–02:40 UTC on 09-20, pinned to cpus 12–15 but
building images and starting containers on the shared daemon). Same shape as
the 24 h soak's five bad seconds, which `journalctl -u docker` tied to image
extraction: a whole-host stall, disclosed, not a runtime defect. 0.00005 % of
the requests in the run.

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

## Three replicas (`p6/replicas.sh three`, 2026-09-23 17:25–17:32 UTC)

`SUPPORTED.md` says N replicas behind a balancer, and every campaign before
this one ran exactly two — so "N" rested on an induction from a single step.
The third replica joins through a compose profile; the properties N > 2 could
break are the shared ones (the queue claim, the migration lock, cron
exclusivity, and a rolling restart that has to keep a quorum serving).

Same shape as the two-replica campaign, on the **published 0.0.8 images**
(`ghcr.io/gmedia/usai:0.0.8` with the application layered on), pinned to cpus
12–15 beside the running 24 h soak (disclosed co-tenant, cpus 0–11; the p99
maxima below carry that).

| Scenario | Pass rule | Result |
|---|---|---|
| HTTP, 2 replicas, 60 s | something served, 0 × 5xx, both created worlds | **58 327 ok**, 0 × 4xx/5xx/503, worlds 29 184 / 29 183, p99 median 29.4 ms |
| Queue, 1 000 invoices | exactly 1 000 deliveries, both consumed, 0 dead, 0 pending | **1 000 (sink saw 1 000)**, app 580 / app2 420 |
| Two `migrate` jobs at once | both exit 0, three files applied once | exit 0/0, 3 rows |
| Cron on one replica | true on app, false on app2 | as expected, `usai_scheduler{cron}` 1/0 |
| Rolling restart under load (20 SSE/WS held) | 0 × 5xx, held connections closed `1012`/ended | **62 905 ok, 0 × 5xx**, ready again 3 s each, closes 10 × `1012` + 10 × SSE ended |
| One replica `kill -9` under load | ≤ 16 × 5xx, every message delivered, nothing pending | **4 × 5xx** (in flight on the dead replica), 4 claims reclaimed, 300 deliveries of 300 (sink 304 — at-least-once) |
| **Three:** HTTP, 45 s | all three created worlds, 0 × 5xx | **38 984 ok**, worlds 13 004 / 13 005 / 13 005 — the proxy's round robin is even across three |
| **Three:** queue, 600 invoices | exactly 600 deliveries across three consumers | **600**, app 182 / app2 180 / app3 238, 0 dead, 0 pending |
| **Three:** rolling restart of all three under load, one at a time | 0 × 5xx | **77 265 ok, 0 × 5xx** over 90 s |
| **Three:** three concurrent `migrate` jobs on a fresh database | none fails, nothing applied twice | 3 applied, **0 applied twice**, 0 failed |

**11 of 11.** Nothing about the shared state changed shape at three: the
claim is still exactly-once per message, the advisory lock still serializes
three migrators, cron is still one replica's job, and a restart of every
replica in sequence costs no 5xx.

The run before this one (16:42 UTC, same code) reported four failures and
four passes, and **all eight verdicts were worthless**: the tenant token came
back empty, so every one of its 1.5 M requests was a 401. The pass rules —
no 5xx, no errors, both replicas created worlds — are all satisfied by
refusals, because a rejected request creates a world too. The harness now
requires that something was *served* before any load scenario can pass, and
`up` fails loudly when it cannot get a token
(`docs/measurements/2026-09-23-floor-accounting.md` is the other half of the
same lesson from the same day).

## Queue throughput (`scripts/qualification/queue/run.sh`, 2026-09-20; re-run on the idle host 2026-09-23)

What the PostgreSQL-backed queue does per second. Application: one consumer
doing the smallest real work (one `INSERT` recording the message), a
publisher command, a producer route. Host: VM 47, apps pinned to cpus 12–15
**shared with `bench-postgres`** (PostgreSQL 18 in Docker, `fsync` on, a
≈12 ms commit on this disk) and the 72 h soak as co-tenant on cpus 0–11.
Per cell: 20 000 messages published first (32 in flight from one world), then
the consumers started and the drain timed from launch to the last `done`.

Four runs during the day, each after a fix the previous one exposed:

| Run | Change | c=4 × 1 | c=8 × 1 | c=16 × 1 | c=4 × 2 | c=8 × 2 | c=16 × 2 | publishes/s (32 in flight) |
|---|---|---|---|---|---|---|---|---|
| 1 | as it was | 93 | 128 | 205 | — | — | — | 320–370 |
| 2 | **publish prepared the schema on every call** (`CREATE TABLE IF NOT EXISTS` + `CREATE INDEX IF NOT EXISTS` per message, two DDL statements and their catalog locks) → once per database per process | 96 | 164 | 205 | 157 | 206 | 242 | 1 100–1 500 |
| 3 | **the claim sorted every ready row** (`ORDER BY id LIMIT 1` over `(topic, available_at)` → 13 ms per claim at 20 000 ready) → `usai_queue_claim (topic, id) WHERE state = 'ready'`, three buffer reads whatever the backlog | 108 | 215 | 388 | 204 | 443 | 658 | 990–1 500 |
| 4 | **the queue's own marks committed synchronously** (claim, done, retry: two fsyncs per message beside the handler's) → `synchronous_commit = off` for those statements only (a lost mark is one redelivery, which at-least-once already allows); an empty claim retries once before sleeping | **209** | **460** | **712** | **473** | 291 † | **956** | 1 100–1 650 |
| ceiling | run 4 with `synchronous_commit = off` on the whole database (engineering ceiling, not a recommendation) | — | 1 016 | — | — | — | — | 5 450 |

† one noisy cell (the two instances and PostgreSQL share four cores with
the load generator); the other five cells of run 4 scale as expected.

**Re-run on the idle host** (2026-09-23 03:22–03:29 UTC, the soak and its
deployment gone, apps pinned to cpus 0–7, `bench-postgres` on 12–13, same
20 000 messages per cell, commit `8acb9ed`):

| | c=4 × 1 | c=8 × 1 | c=16 × 1 | c=4 × 2 | c=8 × 2 | c=16 × 2 |
|---|---|---|---|---|---|---|
| msg/s | 277 | 274 | **1 032** | 687 | 1 036 | **1 198** |
| drain s | 72.2 | 73.1 | 19.4 | 29.1 | 19.3 | 16.7 |
| publishes/s (32 in flight) | 1 603 | 1 800 | 1 700 | 1 841 | 1 802 | 1 715 |
| producer route, 16 clients | 1 219 | 1 121 | 969 | 1 244 | 1 047 | 850 |

Both instances take their half (`i1: 9 985, i2: 10 015` at c=4 × 2): the
work is shared without configuration, as the two-replica campaign found.
Against run 4 on the busy host every cell is faster — c=16 × 1 is 712 →
1 032 msg/s and the publisher is 1 650 → 1 800 per second — which is what
an idle host should do.

One thing in this table is **not explained**: c=4 × 1 and c=8 × 1 are the
same (277 and 274 msg/s) while c=16 × 1 is 3.8× either. Each cell built its
own artifact with its own `concurrency` (checked: `app-c4`, `app-c8`,
`app-c16` carry 4, 8 and 16), so the cells are what they say they are; the
consumer does one `INSERT` per message and the pool is 16. Doubling the
consumers from 4 to 8 bought nothing and doubling again bought everything,
which no model here predicts. Recorded as measured, to be chased with the
per-phase profile on the next queue campaign rather than explained away. **Chased and closed on 2026-09-23** (`2026-09-23-saturation-and-queue.md`): on the idle host the same cells scale 432 → 741 → 1 107 msg/s, so the plateau was that run's, not the consumer loop's.

**The plateau did not reproduce off the VM** (2026-09-23, the WSL2 dev box:
16 threads, PostgreSQL 18 on the same machine and the same disk, 5 000
messages per cell, commit `70601d2`). The absolute numbers are a different
machine's and mean nothing next to the table above; the *shape* is the point:

| | c=4 × 1 | c=8 × 1 | c=16 × 1 |
|---|---|---|---|
| msg/s, `synchronous_commit=on` | 186 | 266 | 445 |
| msg/s, `synchronous_commit=off` | 247 | 361 | 585 |

Doubling the consumers bought 1.4× and then 1.6–1.7×, twice, with and
without durable commits — so **c=4 ≈ c=8 is not a property of the consumer
loop**, and the VM cells need re-running (the run's first two cells are the
suspects: nothing in the harness pins them to an idle moment). What both
machines do show is that four times the consumers buy about 2.4× the
throughput, and that the ratio is the same with `synchronous_commit` off:
the sublinearity is **not** commit durability, which is worth about a third
of the rate here. The hypothesis to test with the per-phase profile is the
claim itself — `ORDER BY id … FOR UPDATE SKIP LOCKED LIMIT 1` makes every
claimant walk past the rows its siblings hold, so the claim grows more
expensive as claimants are added. Not acted on: measuring it is the next
campaign's job, not this report's.

Producer route (`POST /enqueue`, one message per request, consumers up):
10–12 ms per publish for one sequential client — a fresh world plus one
synchronous commit — and 570–900 publishes/s at 16 clients; 3.5 ms / 1 650
publishes/s without fsync. Claim-to-run latency in these cells is the backlog
(everything was published first), not the queue's reaction time; the
consumers poll every 250 ms when the topic is empty and claim at once while
it is not.

Reading. The queue is bound by **the database's commit latency times the
commits per message**, which is now one (the handler's own write) plus
round trips: on a 12 ms-fsync disk that is ≈45–50 messages per second per
worker, linear in `concurrency` and in instances; on a disk with a 1 ms
fsync, expect roughly ten times that. The runtime's own cost per message
(a fresh world, the claim and the mark) is what the `synchronous_commit =
off` row measures: ≈8 ms of worker time, ≈1 000 messages/s for eight
workers on one instance. 5× over the day's first run, all of it in the
queue's use of the database, none of it in the model.

## Mixed versions on one queue table (2026-09-23, developer machine)

`SUPPORTED.md` and `runbooks/systemd.md` promise that a 0.0.5 and a 0.0.6
instance can serve behind one proxy during a roll — "they share nothing but
PostgreSQL, and the queue table and the migration ledger are stable across
versions". Measured, because 0.0.6 adds a column to `usai_queue`:

| Step | Result |
|---|---|
| The 0.0.5 schema created verbatim (no `request_id`, one index), a message waiting in it, then a 0.0.6 runtime prepares the schema | the column and the two indexes are added in place, the waiting row is untouched and keeps `request_id` NULL (regression test `the_queue_table_upgrades_in_place_and_the_previous_version_keeps_working`) |
| The 0.0.5 statements (publish, claim, done) run verbatim against the upgraded table | all three work; a 0.0.5 consumer never sees the new column |
| 20 messages published by the 0.0.6 CLI, consumed by a **0.0.5 runtime** | 20/20 consumed, `state = done` |
| Both runtimes consuming the same topic while the **0.0.5 CLI** publishes 100 with 8 in flight | 100 distinct messages processed exactly once — 92 by the 0.0.5 instance, 8 by the 0.0.6 one — no duplicate rows, no ordering requirement broken |

One message of the 120 ended as a dead letter (`consumer lost: claimed by
rev1:bench.work:6 …`, the 0.0.5 `locked_by` format) although its handler had
run: under the publisher's burst the 0.0.5 consumer's `done` mark did not
land, the row stayed `processing`, and the 0.0.6 sweeper dead-lettered it 40 s
later because the application declares no `retry` (`maxAttempts` defaults to
1). The mark was discarded with `let _ =` on both versions — a failed mark
was silent. **Fixed in 0.0.6**: the mark is retried once, synchronously, and a
second failure is logged with the consequence; the runbook says how to tell
the two kinds of `consumer lost` apart (`runbooks/queue-dead-letter.md`).

## Not done here

Three or more replicas, and a replica set across hosts (the queue and the
migration lock are PostgreSQL-side and do not care; the proxy configuration
does). A soak whose workload does not accumulate data — the 72 h run above
measured the runtime's invariants cleanly but its throughput curve is the
PostgreSQL write path, so "steady-state throughput over days" is still
unmeasured for a bounded dataset.
