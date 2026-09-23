# Above the sweep's ceiling: c=64 → 512, and the queue plateau explained
(2026-09-23, VM 47, idle host, commit `2b509d1` = 0.0.8)

The concurrency sweep earlier the same day stopped at c=64 and said so under
"what this run does not measure". Two questions were left open by it, and
both are answered here: **what happens above the ceiling**, and **why the
queue's c=4 and c=8 cells had been identical**.

Server pinned to cpus 0–7, client to 8–11, `bench-postgres` to 12–13; 20 s
per cell; the same six workload classes as the sweep.

## Saturation: the curve above c=64

Usai, one process, `--max-worlds` at its default:

| class | c=64 | c=128 | c=256 | c=512 |
|---|---|---|---|---|
| **A** hello | 15 238 rps, p99 7.8 ms | 15 019, p99 17.1 | 15 034, p99 37.5 | 14 976, p99 95.2 |
| **B** validation | 8 440, p99 16.4 | 8 606, p99 37.2 | 8 640, p99 79.6 | 8 575, p99 99.4 |
| **C** DB read | 7 469, p99 12.8 | 7 166, p99 25.2 | 7 094, p99 43.8 | **refused** |
| **D** insert | 2 550, p99 61.9 | 2 766, p99 126.2 | 2 630, p99 227.8 | 2 957, p99 379.1 |
| **E** transaction | 4 218, p99 50.7 | 4 657, p99 70.1 | 4 631, p99 121.1 | **refused** |
| **F** auth + read | 5 837, p99 16.5 | 5 900, p99 27.1 | 5 599, p99 56.8 | **refused** |

**Throughput is flat and latency is linear in the concurrency.** From c=64 to
c=256 every class holds its rate within a few per cent while p99 grows by
about the factor the concurrency does — which is what a saturated server
that does not thrash looks like: the queue is the client's, not the
server's. Nothing degrades, nothing collapses, and there is **not one 5xx**
below c=512.

**At c=512, three classes are refused rather than served slowly.** C, E and F
answer 0.8–1.2 M refusals in 20 s (the "rps" in those cells counts refusals,
which are cheap). D, whose requests are slow enough that fewer are ever in
flight at once, stays clean. That is the documented overload contract —
*refusal is immediate, never a queue inside a world* — and the server log
for the whole run contains **no ERROR line and no fault**, so those answers
were refusals rather than failures.

What this run could not do is separate a `503` from another 5xx: the bench
client counted both together. **It does now** (`s503` in both clients), so
the next run says which budget was full — worlds (`capacity_exhausted`) or
the database pool (`unavailable`) — instead of leaving it to inference.

**Memory follows concurrency, not requests.** The classes that touch
PostgreSQL sit at ≈1 GiB of RSS from c=64 upward; hello and validation stay
at 78–91 MiB even at c=512, because their worlds are short-lived and no pool
is held. This is ADR-0019's lever 2 in one line: the plateau is the peak
concurrency ever reached, and it does not come back down
(`docs/upstream/wasmtime-idle-decommit.md` is the proposal that would let it).

For context, the same cells for the comparators at c=64 → 512 on hello: one
Node process 10 659 → 10 845, an eight-worker cluster 108 437 → 111 154.
Both are equally flat; the sweep's conclusions about *where* each runtime
sits are unchanged above the old ceiling.

## The queue plateau: not the consumer loop

The 2026-09-23 queue re-run had reported c=4 × 1 and c=8 × 1 as identical
(277 and 274 msg/s) while c=16 × 1 was 3.8× either, and the report recorded
it as unexplained. On the idle host, with the same 20 000 messages per cell:

| | c=4 × 1 | c=8 × 1 | c=16 × 1 | c=4 × 2 | c=8 × 2 | c=16 × 2 |
|---|---|---|---|---|---|---|
| msg/s | 432 | 741 | 1 107 | 308 | 1 146 | 1 581 |
| drain s | 46.3 | 27.0 | 18.1 | 65.0 | 17.5 | 12.7 |

**The plateau is gone**: doubling the consumers buys 1.7× and then 1.5×. The
same shape was measured independently on the development box the same day
(186 → 266 → 445 msg/s with durable commits, 247 → 361 → 585 without), so
the earlier reading was a property of that run, not of the consumer loop —
the first two cells of a run are the ones nothing pins to an idle moment.

One cell is still odd: **c=4 × 2 (308 msg/s) is slower than c=4 × 1 (432)**,
while every other two-instance cell is faster than its single. Eight
consumers across two processes should not be slower than four in one. It is
recorded as measured; the per-phase profile is the next queue campaign's
job.

What both machines agree on, and the sweep did not ask: four times the
consumers buy about 2.4× the throughput, and the ratio is unchanged with
`synchronous_commit=off`. The sublinearity is therefore **not** the commit —
the claim (`ORDER BY id … FOR UPDATE SKIP LOCKED LIMIT 1`) makes every
claimant walk past the rows its siblings hold, and that is the hypothesis to
test.

## What this run does not measure

- **Anything above c=512**, and nothing about a client that is itself the
  bottleneck: 512 connections come from one box pinned to four cores.
- **Which refusal** the c=512 cells produced (see above; fixed for next
  time).
- **The comparator floors**, which failed for reasons of our own in part 1 of
  this campaign and are re-running: the node cells mounted a directory whose
  `bin/node` was a symlink into another directory, and our own template cell
  wanted a database nobody had created. Both are fixed in the harness; the
  php cells **are** valid, and their limits are now verified against the
  daemon rather than assumed (`appliedMemoryBytes`, `appliedNanoCpus` in each
  `result.json`).
