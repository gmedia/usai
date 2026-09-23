# A floor is only true if the box was billed for it (2026-09-23)

A floor cell says: *this application fits on a machine this small.* It is a
claim about a bill, and it is worth exactly as much as the accounting behind
it. This is the day the accounting turned out to be wrong, how it was caught,
and what the harness does now.

## The impossibility

The comparator floors were re-run on VM 47 (`p8e-floors2-20260923T153421Z`)
after part 1's node cells had died on a dangling mount. They came back like
this:

| cell | box | OOM-killed | RSS peak the sampler saw |
|---|---|---|---|
| `node-48m-1c-0w` | 48 MiB | no | **88.7 MiB** |
| `node-64m-1c-0w` | 64 MiB | no | 83.0 MiB |
| `node-96m-1c-0w` | 96 MiB | no | 84.8 MiB |
| `node-128m-1c-0w` | 128 MiB | no | 85.8 MiB |

Each cell verified its limits against the daemon first
(`HostConfig.Memory` = 50 331 648 for the first row), and `--memory-swap`
equals `--memory`, so there is nowhere to swap to. **A 48 MiB box cannot hold
88 MiB.** The four cells also answered within 3 % of each other at every size,
which is the signature of a limit that binds on nothing.

## Why

Page cache is charged to the cgroup that **first faults a page in**, and it
stays charged there until it is reclaimed. The harness bind-mounted the host's
`node` binary into the box — and the harness itself runs that same binary, on
the host, to prepare the database and to drive load. By the time the cell
started, the binary's text was already resident and already billed to the
host. The box mapped the same inode and paid nothing for it. What it *did* pay
for — the heap, the stacks, the sockets — fits in 48 MiB comfortably, so
nothing was ever killed.

The sampler, reading `/proc/<pid>/smaps_rollup`, saw the whole resident set
and reported it faithfully. Both numbers were right. Only their combination
was impossible, and nothing in the harness was looking at the combination.

**This is not a comparator-only defect.** Our own boxed cells bind-mounted
`usai` and the artifact exactly the same way, and the campaign runs `usai`
on the host before every cell (`build`, `db migrate`). Every floor in
`2026-09-20-p8e-efficiency.md` — including the **192 MiB supported floor** in
`SUPPORTED.md` — was measured on a machine that already had the runtime
resident. The direction of the error is known: it can only have made the
floors look smaller than they are.

## What the harness does now

1. **The binary and the application are layers of an image** the run builds
   (`fleet.sh` → `build_boxes`), never bind mounts. The box gets its own
   inode, so every page it touches is charged to it — and this is also what a
   deployment actually looks like: `ghcr.io/gmedia/usai` carries the binary in
   a layer.
2. **Each cell records what the kernel charged**, not only what the sampler
   could see: `memory.peak`, `memory.current` at the end, `memory.events`
   `max` (how often the box hit its ceiling and had to reclaim) and
   `oom_kill`. A cell that never OOMs but sits against its ceiling is passing
   on reclaim, and the report can now say so.
3. **`pagecache-check.sh`** runs the experiment on its own: the same process in
   the same box, once bind-mounted and once as an image layer, printing the
   resident set beside the charge. Thirty seconds, no campaign, and it is how
   to tell whether a floor harness on any other machine is honest.

The PHP cells keep a bind mount of the application's `.php` files (a few
kilobytes through nginx and FPM); the interpreter comes from its own image
layer, so the bill is right where it matters.

## Status

**The floors of 2026-09-20 and 2026-09-23 are void.** The corrected run is
queued behind the 24 h bounded soak on VM 47 and covers `node` 48/64/96/128,
`template` 128/192/256 and `hello` 48/64, all at 1 vCPU. Until it lands:

- `SUPPORTED.md`'s host envelope row is marked as under re-measurement.
- No floor number, ours or a comparator's, should be quoted from this repo.

Density (§5 of the P8E report) and the residency finding (§4) are unaffected:
those are bare processes on the host, where the host *is* the payer.

## What this run does not answer

- **How much the correction moves the numbers.** The direction is known, the
  magnitude is not, and guessing it here would repeat the mistake this
  document is about.
- **Whether other campaigns share the defect.** Every campaign that puts a
  number on memory was re-read. The density campaign runs both sides as bare
  processes on the host, where the host *is* the payer and PSS is the right
  instrument; P5/P6 and the soak run the application from a built image, so
  the bill lands in the container; the sweep and the saturation run measure
  throughput, latency and error counts, none of which are charged to a
  cgroup. The boxed floor cells were the only place a bind-mounted binary met
  a memory limit.
