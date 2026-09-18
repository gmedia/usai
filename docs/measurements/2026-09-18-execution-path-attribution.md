# Execution-path attribution (P1) — 2026-09-18

Engineering evidence for ADR-0016's open question: *where does a Wasm world's
time go?* Measured, not asserted. Numbers are from two machines; the VM is the
one to reason from (same hardware as the research's EXP-012B).

- **VM:** Linux 7.0.0-31-generic, 16 × Intel Xeon E5-2680 v4 @ 2.40 GHz, 31 GB,
  `perf` available. Production repo synced to `~/usai-prod`; the research
  checkout at `~/usai` was read (the sealed `-Oz` core) and not modified.
- **WSL2:** the developer machine (Hyper-V); its speed drifted 2× during the
  session, so only ratios from it are cited.
- Build: release, Rust 1.98.1, Wasmtime 48, core `-O3`
  (`c4e58003…`), research core `-Oz` (`6b33cb45…`, EXP-011C).
- Harness: `crates/usai-runtime/tests/profile_matrix.rs` +
  `scripts/p1-attribution.sh`; fixture `tests/fixtures/bench-app` (one bundle,
  many workloads, so a row isolates the handler, not the image).

## 1. Hello on the VM (`usai bench`, `examples/hello`, 5 s)

```text
engine    c    req/s    p50      p90      p99
wasm      1      369    2.62 ms  2.96     3.23
wasm     16    2 497    6.24     6.82     9.05
wasm     64    2 471   25.64    34.70    42.49     ← saturates at c=16
quickjs   1       86   11.48    11.99    13.42
quickjs  16    1 088   14.37    15.53    21.12
quickjs  64    1 094   57.41    79.00    96.61
```

Still slow relative to the research's sub-millisecond p50, and the Wasm
engine stops scaling at 16 concurrent clients on 16 cores. Both are explained
below.

## 2. Phase ledger (VM, `Runtime::invoke`, ms per request, n = 300)

`USAI_PROFILE=1` makes each engine time its own guest calls. Columns:
`total` = wall time of `Runtime::invoke`; `create` = admission + instantiate;
`run` = invoke → outcome; `outside` = total − create − run − retire (instance
drop / slot reset + bookkeeping); `unacct` = run outside timed guest calls;
`faults` = minor page faults per request.

### Wasm, `-O3` core (production)

```text
workload       total  create     run  outside  invoke.eval invoke.jobs outcome pending  unacct  faults
empty          0.901   0.021   0.687   0.19        0.480     0.016     0.083   0.086   0.022    95
constant       0.912   0.021   0.695   0.19        0.481     0.016     0.087   0.088   0.023    95
loop 1e5      12.087   0.024  11.841   0.22       11.607     0.018     0.097   0.094   0.025    95
objects 5k     9.271   0.023   8.900   0.35        8.691     0.018     0.092   0.076   0.023   265
json 200k      3.105   0.022   2.737   0.35        2.545     0.017     0.088   0.063   0.024   223
host x1        2.399   0.023   2.039   0.34        0.755     0.005     0.116   0.069   1.046   136
host x8       10.692   0.024  10.321   0.35        0.759     0.006     0.332   0.078   8.660   137
sdk-only       1.134   0.021   0.820   0.29        0.533     0.076     0.107   0.077   0.027   116
zod/mini       1.522   0.022   1.165   0.33        0.557     0.407     0.106   0.065   0.030   154
zod            2.717   0.022   2.334   0.36        0.524     1.639     0.084   0.058   0.028   235
zod x1         2.883   0.022   2.490   0.37        2.321     0.016     0.074   0.055   0.023   279
zod x10        3.209   0.023   2.807   0.38        2.630     0.017     0.078   0.057   0.024   279
mini x1        1.485   0.022   1.126   0.34        0.916     0.030     0.090   0.067   0.023   160
mini x10       1.727   0.022   1.372   0.33        1.159     0.031     0.091   0.068   0.023   160
crud list      4.102   0.023   3.693   0.38        0.523     2.738     0.275   0.058   0.099   295
crud create    4.232   0.023   3.808   0.40        0.591     3.029     0.095   0.057   0.035   315
```

(`instantiate.*` sub-phases: store 0.003, instance 0.010, exports 0.000,
seed 0.005 — the P0 result holds: instantiation is not where the time is.)

### Wasm, `-Oz` research core (same host, same harness)

```text
workload       total    run   invoke.eval invoke.jobs  faults      vs -O3
empty          0.994   0.769     0.538      0.021        92     +10 %
loop 1e5      16.008  15.744    15.502      0.022        92     +32 %
objects 5k    12.867  12.335    12.081      0.024       264     +39 %
json 200k      3.816   3.458     3.242      0.020       217     +23 %
zod            3.086   2.687     0.559      1.962       237     +14 %
crud list      4.907   4.497     0.542      3.398       282     +20 %
```

`-Oz` costs 19–49 % more instructions for the same work (perf, below). It is
not the main lever.

### Native QuickJS (reference, not production)

```text
workload       total  create     run   invoke  outcome pending  unacct  faults
empty         14.375  11.539   0.101    0.056   0.018   0.007   0.020     0
loop 1e5      26.576  11.654  12.050   11.999   0.022   0.008   0.022     0
objects 5k    19.199  11.570   4.684    4.634   0.021   0.007   0.021     0
json 200k     15.045  11.397   0.882    0.840   0.016   0.007   0.019     0
host x1       15.507  11.418   1.300    0.088   0.024   0.008   1.154     0
sdk-only      14.964  11.923   0.137    0.075   0.028   0.007   0.027     0
zod/mini      15.075  11.944   0.195    0.138   0.026   0.007   0.024     0
zod           15.448  11.845   0.677    0.621   0.025   0.007   0.024     0
zod x1        15.432  11.832   0.651    0.606   0.017   0.007   0.021     0
zod x10       15.737  11.953   0.819    0.774   0.017   0.007   0.021     0
crud list     16.192  11.838   1.399    1.086   0.261   0.006   0.046     0
```

`create` = 11.5 ms is the ADR-0015 initialization tax (runtime + SDK + zod
evaluated per world). Curiously the pure loop is *not* faster native on this
CPU (12.0 vs 11.6 ms); on WSL2 native was 2.4× faster. Not investigated —
native is the reference engine, not a production candidate.

## 3. Hardware counters (VM, `perf stat`, per request, delta of n=20 000 vs n=1)

```text
config    workload   instructions    cycles   minflt  ctxsw   IPC
wasm-O3   empty         2 123 053  2 798 636    95.1   3.68   0.76
wasm-O3   zod           7 699 052  8 152 343   240.0   4.27   0.94
wasm-O3   crud list    15 228 209 12 222 405   302.1   4.36   1.25
wasm-Oz   empty         2 534 029  2 935 556    92.0   3.80   0.86
wasm-Oz   zod          10 781 193  9 218 859   251.1   4.22   1.17
wasm-Oz   crud list    22 692 824 15 232 019   293.2   4.64   1.49
quickjs   empty        67 800 775 42 060 127     0.1   4.22   1.61
```

The native engine spends 68 M instructions to run an empty handler — 32× the
Wasm image — which is the ADR-0015 initialization tax in one number.
Major faults 0 everywhere. ~4 context switches per request come from the
watchdog handshake around guest calls and completion routing. IPC below 1 on
the empty world is the page-fault signature.

## 4. Where the Wasm world's time goes (VM, hello-shaped `zod` row, 2.7 ms)

```text
 1.64 ms  zod first-parse initialization.  zod 4 builds schema internals lazily
          on the first parse; every world is fresh from the image, so every
          world pays it.  Evidence: zod x10 − zod x1 = 0.31 ms → 0.03 ms per
          extra parse; the first costs 1.8 ms.  zod/mini: 0.40 first, 0.03 after.
 ~1.0 ms  first-touch minor page faults (240 per request, ~4.4 µs each here).
          Evidence: with the slot reset changed so nothing is decommitted
          (USAI_WASM_PAGEMAP_SCAN=0, USAI_WASM_KEEP_RESIDENT=16777216) faults
          drop to 0 and `run` falls 2.39 → 1.41 ms (empty: 0.71 → 0.29 ms),
          while `outside` (memcpy reset) rises 0.19 → 0.61 ms.
 0.15 ms  `__usai.invoke` eval floor without faults (compile the snippet +
          SDK dispatch); native does the same in 0.056.
 0.10 ms  `outcome` + `pending` evals without faults (0.057 + 0.044).
 0.19 ms  instance drop / slot reset (default config).
 0.02 ms  create (admission + instantiate + seed).
 0.03 ms  unaccounted inside run.
```

The sum matches `total` within the noise of the row. Nothing is unexplained.

Why the faults exist: the image's linear memory is 1.44 MiB (hello). Each
request grows the heap past the snapshot (malloc's top chunk is nearly empty
at snapshot time): with the scan off and 2 MiB kept resident there are still
133 faults, so pages beyond 2 MiB are touched every request. On slot reset
Wasmtime 48 scans the pagemap for dirty regions with a hard cap of **32
regions** (`MAX_REGIONS` in `runtime/vm/sys/unix/pagemap.rs`) and
`madvise(DONTNEED)`s everything after the scan's end — clean pages included —
which then refaults in the next world. The observations are consistent with a
QuickJS heap dirtying more than 32 runs: `keep_resident` has no effect while
the scan is on (2 MiB vs 16 MiB, same 95 faults), and turning the scan off
with everything kept resident removes every fault.

Why it stops scaling at c=16: `perf record` of the default config at c=16
shows **19.6 % in `smp_call_function_many_cond`** plus `flush_tlb_func`,
`native_flush_tlb_local`, `zap_present_ptes` — TLB-shootdown IPIs from the
per-world `madvise`/`mprotect` broadcast to every core running one of our
threads. With decommit avoided the IPIs vanish and the same bench gives:

```text
config                              c=1 p50   c=16 req/s  c=16 p50  c=64 req/s
default (pagemap, keep 2 MiB)       2.62 ms      2 291     6.29 ms    2 343
no decommit (keep 16 MiB, no scan)  2.19 ms      4 437     3.21 ms    4 283
```

1.9× throughput from a runtime flag. The remaining ceiling (16 cores × 1/2.19
ms ≈ 7 300 ideal vs 4 400) is now the reset memcpy — `perf` shows ~35 % in
libc `memmove`/`memset` at c=16 — i.e. memory bandwidth, not a lock.

Host round trips: `host x1` costs 1.05 ms outside the guest on **both**
engines — that is tokio's 1 ms timer resolution for `sleep(0)`, not the
bridge (`deliver.complete` 0.011 + `deliver.jobs` 0.035 per completion).

## 5. Levers, ranked by the ledger (decisions, not yet done)

| # | Lever | Expected on hello (VM) | Kind |
|---|-------|------------------------|------|
| 1 | Slot reset without decommit: reset only dirty pages, never `madvise` inside the image; needs Wasmtime's `MAX_REGIONS` lifted (upstream or vendored patch) — or, until then, `pagemap_scan=false` + `keep_resident` ≥ memory size (memcpy floor ≈ 0.4 ms, bandwidth-bound at scale) | −0.6 ms p50, ×1.9 throughput at c=16 today; more with the real fix | host config / upstream |
| 2 | Warm validators before the snapshot (SDK `warm()` runs each declared schema once during image build) | −1.6 ms on zod paths, −0.4 on zod/mini | **image content — stop-and-surface** |
| 3 | Pre-grow the heap in the image (allocate + free slack before snapshot) so requests do not `memory.grow` | fewer fresh pages; interacts with 1 | image content — stop-and-surface |
| 4 | `sleep(0)` / timer 0 → `yield_now` instead of the timer wheel | −1 ms per zero-delay timer | host |
| 5 | Fold `outcome` + `pending` into one eval; later a direct-call ABI instead of `eval` (core export) | −0.05 now; −0.1–0.2 with a core change | host / **core ABI — stop-and-surface** |
| 6 | Watchdog handshake per guest call (4 context switches / request) | tens of µs; matters at c=16 | host |

Not a lever: `-O3` vs `-Oz` (already `-O3`; 10–40 % on CPU-bound rows only),
Wasmtime 47 vs 48 (attribution did not stall; skipped by plan), the
instantiate path (0.02 ms).

## 6. WSL2 cross-check (same harness, same day)

Same shape, different absolute numbers: Wasm `empty` 0.62 ms total (95
faults), `zod` 2.10 (invoke.jobs 1.31), `zod x1` 1.99 vs `zod x10` 2.17;
native `zod` 0.50. Earlier session numbers ("world run 4.8 ms") were taken
while the machine ran 2.3× slower (native 18 ms vs 7.6 ms create) and are
consistent with this ledger.

## 7. After P2 (same VM, same harness, commit ff2688f)

Levers applied: slot reset root fix (vendored Wasmtime patch: complete
pagemap traversal, `vendor/README.md`), validator preparation before the snapshot
(`packages/usai/src/runtime/prepare.ts`, callback-free), zero-delay timer →
yield, lock-free watchdog. Heap slack (#3) and the ABI change (#5) were not
needed.

### Hello (`usai bench`, `examples/hello`, 5 s)

```text
engine    c    req/s   p50      p90     p99     before (P1)
wasm      1      658   1.52 ms  1.54    1.59    369 req/s, 2.62 ms
wasm     16    9 763   1.58     1.80    2.55    2 497 req/s, 6.24 ms
wasm     64    9 788   6.45     8.70   10.82    2 471 req/s, 25.64 ms
```

c=16 throughput ×3.9; p50 at c=16 now equals p50 at c=1 — the cross-core
interference is gone. c=64 is flat at c=16's rate because the in-process
bench client shares the 16 cores; a separate load generator is needed to
find the next ceiling (Q8 stays open on that point only).

### Phase ledger (ms per request, n = 300)

```text
workload       total    run   invoke.eval invoke.jobs outcome pending  faults    before (total)
empty          0.376  0.230     0.114      0.015      0.052   0.036      0        0.901
constant       0.387  0.240     0.116      0.015      0.057   0.038      0        0.912
loop 1e5      11.325 11.146    11.007      0.017      0.064   0.041      0       12.087
objects 5k     6.631  6.324     6.189      0.016      0.062   0.039      0        9.271
json 200k      1.934  1.628     1.503      0.016      0.056   0.038      0        3.105
host x1        0.617  0.311     0.150      0.005      0.071   0.037      0        2.399
host x8        1.061  0.750     0.154      0.005      0.195   0.039      0       10.692
sdk-only       0.590  0.279     0.121      0.040      0.062   0.037      0        1.134
zod/mini       0.653  0.337     0.123      0.092      0.064   0.037      0        1.522
zod            1.052  0.713     0.128      0.457      0.067   0.039      0        2.717
zod x1         1.507  1.148     1.018      0.016      0.058   0.038      0        2.883
zod x10        1.779  1.420     1.283      0.017      0.062   0.041      0        3.209
crud list      2.604  2.218     0.135      1.679      0.269   0.041      0        4.102
crud create    2.470  2.096     0.134      1.816      0.079   0.040      0        4.232
```

`zod x1`/`zod x10` parse a schema that is *not* a declared contract (it is
private to the handler), so it is not prepared — the row now serves as the
control: a prepared contract (`zod`, two schemas) costs 0.46 ms of jobs
against 1.0 ms for one unprepared first parse. What remains in the prepared
path is the first *successful* parse (result shapes, inline caches), which
cannot be warmed without running the validator for real.

### Hardware counters per request

```text
workload   instructions   cycles   minflt  ctxsw     before
empty         1 061 294  1 131 849   0.2    2.2    2.12 M / 2.80 M / 95 / 3.7
zod           3 149 146  2 784 506   0.0    2.4    7.70 M / 8.15 M / 240 / 4.3
crud list    11 849 248  7 093 552   0.1    2.9   15.23 M / 12.22 M / 302 / 4.4
```

The remaining ~2 context switches per request are the completion channel
and the tokio hand-off, not the watchdog.

### What the world's time is now (VM, `zod` row, 1.05 ms)

```text
 0.46  prepared validation × 2 + handler (first successful parse path)
 0.13  invoke eval (compile the snippet + SDK dispatch)
 0.11  outcome + pending evals
 0.13  instance drop / slot reset (dirty pages only)
 0.02  create
 0.02  unaccounted
 0.18  outside (invoke bookkeeping, channel)
```

The eval floor (0.24 ms of 1.05) is now the largest fixed cost and the next
candidate (#5), after a soak proves the new reset path stable.

## 8. Soak (VM, 1 h, `usai bench -c 16 -d 3600`, hello, warmed artifact)

```text
requests      49 140 215 ok, 0 errors (13 650 req/s)
latency ms    p50 1.11  p90 1.37  p99 2.03  max 32.28
worlds        49 140 231 created, 0 live after drain, 0 live ops
RSS           288 420 kB at start → 291 068 kB at the end (+0.9 %; +0.3 % after the first 10 minutes)
throughput    12.4k–14.0k req/s per 10 s window, no trend
```

RSS sampled every 30 s from `/proc/<pid>/status` (359 samples), threads
steady at 37–38. The bench client keeps a fixed-size histogram, so the
process's memory is the runtime's. No fault, no leak, no drift on the new
slot-reset path over 49 M worlds.

## 9. A freshness hole in the reset path, found by the hardening suite

Under a fully loaded developer machine (swap 100 % used) the
`revision_replacement_under_load_loses_no_request` test failed about one
run in three with guest traps in `dlfree` and out-of-bounds accesses inside
`JSON.parse` — a corrupted guest heap. Bisect: never with the pagemap scan
off (memcpy reset), only with it on. Cause: Wasmtime's scan matched dirty
pages only when `PRESENT`; a dirty page the kernel had swapped out
(`WRITTEN | SWAPPED`) was neither reset nor decommitted, so the next world
in that slot started with the previous world's heap bytes. The complete-traversal
patch (§7) widened the window (upstream decommits everything after its
32-region walk, which hid most of it), but the hole is upstream's.

Fix: match `WRITTEN` without requiring residency (`vendor/README.md`, hunk
2). Verified on the research VM with a 2 GB swapfile:
`a_reused_slot_is_fresh_even_after_its_pages_were_paged_out` fails with the
old mask and passes with the new one; the full workspace loop no longer
fails on the swapping developer machine.

Lesson for the thesis: "fresh world" is a property of the reset mechanism,
and the reset mechanism has to be tested under the conditions production
has — memory pressure included.
