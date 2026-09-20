# Comparative benchmark: the same hello endpoint on Usai, Node, Bun, Deno (2026-09-19)

> Superseded for the per-request cost: this run predates P8, which halved the
> hello path (`2026-09-19-p8-parity.md`); its "≈1 ms is the price of the model"
> framing was wrong and is corrected there. Kept for the concurrency curve and
> the disclosure.

`scripts/qualification/bench/run.sh` on the research VM (16 × Xeon E5-2680
v4): `GET /hello/world` answering `{"hello":"world"}` with a 400 for names
over 40 characters — Usai 0.0.5 release binary serving the `examples/hello`
artifact (`usai run --artifact`), Node 24 `http`, Bun 1.4 `Bun.serve`, Deno
2.9 `Deno.serve`; one process each, same host, `oha` 10 s per cell. The 24 h
soak (≈1 000 req/s on another port) was running on the same machine, so
every row carries that background load. Engineering numbers, one machine,
one run.

| server | concurrency | req/s | p50 ms | p99 ms | RSS MiB |
|---|---|---|---|---|---|
| usai 0.0.5 | 1 | 826 | 1.13 | 1.95 | 46 |
| usai 0.0.5 | 16 | 5 772 | 2.35 | 8.29 | 109 |
| usai 0.0.5 | 64 | 7 306 | 8.31 | 20.72 | 111 |
| node v24.13 | 1 | 6 912 | 0.13 | 0.30 | 76 |
| node v24.13 | 16 | 14 531 | 0.96 | 2.34 | 80 |
| node v24.13 | 64 | 15 382 | 3.97 | 7.92 | 89 |
| bun 1.4.2 | 1 | 8 094 | 0.10 | 0.57 | 33 |
| bun 1.4.2 | 16 | 32 774 | 0.40 | 1.39 | 42 |
| bun 1.4.2 | 64 | 38 033 | 1.52 | 3.58 | 35 |
| deno 2.9.7 | 1 | 9 448 | 0.09 | 0.19 | 45 |
| deno 2.9.7 | 16 | 35 909 | 0.40 | 1.09 | 45 |
| deno 2.9.7 | 64 | 41 201 | 1.50 | 2.56 | 46 |

## Reading it

A hello endpoint measures per-request overhead and nothing else. Usai pays
about **1 ms per request** here for what the others do not do: a fresh
execution world (reset memory image, no state from the previous request),
boundary validation before the world exists, and an ownership ledger for
every asynchronous operation. That is the **measured fixed cost of the
current v0.0.5 implementation on this host and workload** — not a proven
price of the model. The research representation of the same semantics (R3)
reached +51 µs p50 over Node/Fastify at c=1 on an HTTP + PostgreSQL
endpoint, and the whole-request invoice (`USAI_PROFILE=1`, P8) attributes
most of today's millisecond to implementation choices: three `eval`-based
guest calls per request, boundary contracts validated twice (host JSON
Schema, then Zod's first-parse path inside every fresh world), the slot
reset. It is where the throughput gap comes from (≈2× below Node, ≈5× below
Bun/Deno at c=64 on this host); P8 (`docs/ROADMAP.md`) is the audit that
removes what the semantics never asked for — and did, the same day: at c=1
on the same host the hello request went from 1.70 to 0.90 ms p50 (CPU 1.45
→ 0.65 ms) and the contract-heavy class from 5.27 to 1.28 ms
(`2026-09-19-p8-parity.md`, with the invoice of what remains). On the same VM without the soak running, the attribution
campaign measured 9.8k req/s at c=16 (`2026-09-18-execution-path-attribution.md`),
so roughly a third of the gap here is the background load.

What a hello benchmark does not show: with real work per request
(PostgreSQL queries, validation, auth) the fixed cost is a smaller share —
the P5 deployment did ≈1 000 req/s of real invoicing traffic at p50 6 ms
through a proxy with 8 clients, limited by the client, not the runtime —
and the failure behaviour under load (503 refusals in microseconds, no
pile-ups, no cross-request state) is the point of the design, not the
req/s column.

Not measured here: PHP, a Rust baseline, and the other runtimes under the
same ownership guarantees (there is no equivalent to compare). Reported as
it came out; the harness is in the repository for anyone to rerun.
