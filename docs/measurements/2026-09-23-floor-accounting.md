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

## The fix that was not one

The first correction was to bake the binary into an image instead of
bind-mounting it, on the theory that the box would then get its own inode and
its own bill. It does get its own inode. It does not get the bill —
`docker build` warms the cache exactly as thoroughly as the host running the
binary did. Measured on VM 47 with a 121 MB `node`, both ways, same box:

| | RSS KiB | charged KiB |
|---|---|---|
| bind mount, warm cache | 41 884 | 8 700 |
| image layer, warm cache | 41 984 | 8 852 |

Thirty-three megabytes of resident memory that nobody charged the container
for, either way. **Where the file comes from is not the variable. Who touched
it first is.** A production host has the same property, incidentally: after
`docker pull`, the layer's pages are charged to the daemon that extracted
them, not to the container that runs them.

## What the harness does now

1. **Every cell drops the page cache for what it is about to run**
   (`evict.py`: `posix_fadvise(POSIX_FADV_DONTNEED)` over the binary, the
   artifact and the comparator's dependencies). It needs no privileges —
   unlike `/proc/sys/vm/drop_caches` — and it leaves the box as the first
   faulter, so the kernel charges it for everything it touches. That is the
   machine a floor is a claim about: one that has never run this application
   before. The files go back to being bind-mounted, because a file on the
   host is one the harness can evict and an overlay layer is not.
2. **Each cell records what the kernel charged**, not only what the sampler
   could see: `memory.peak`, `memory.current` at the end, `memory.events`
   `max` (how often the box hit its ceiling and had to reclaim), `oom_kill`,
   and `coldCache` — whether the eviction actually happened. A cell that
   never OOMs but sits against its ceiling is passing on reclaim, and the
   report can now say so.
3. **`pagecache-check.sh`** runs the experiment on its own: the same process
   in the same box three ways — bind-mounted warm, image layer warm, and
   bind-mounted cold — printing the resident set beside the charge. A minute,
   no campaign, and it is how to tell whether a floor harness on any other
   machine is honest.

The PHP cells are the one place this is still incomplete: the interpreter
lives in an image layer that cannot be evicted without root, so a php floor
is measured warm and says so (`coldCache: false`). It is recorded rather than
claimed.

## And the part that cannot be evicted

Dropping the binary's cache moved the charge from 8.7 MB to 18.9 MB against
42 MB resident. Better, and still 23 MB the box was not billed for: it maps
its libc and friends from the base image, and those pages live in a layer an
unprivileged process cannot evict — with Docker's containerd image store
there is not even a layer path to aim at (`GraphDriver` inspects as `null`).
The whole page cache *can* be dropped, with root, and that is the complete
method (`DROP_CACHES=1`); but it is a host-wide I/O event, and the 72 h
soak's only bad seconds were caused by exactly that, so it is opt-in and
never runs beside another measurement.

Which forces the honest reading of a floor, and it is not the one the
campaign had been using:

> **A floor is read from the resident set, not from the absence of an OOM
> kill.** "It did not get killed at 48 MiB" is only as true as the charging,
> and the charging is optimistic whenever anything else on the host has
> touched the same files. "It held 42 MiB at its peak" is measured directly,
> the same way for every runtime in the comparison, and an operator sizing a
> limit wants that number plus headroom anyway.

So each cell reports `coldCache: full | true | partial` beside its peak, and
a floor is `peak RSS + headroom`. The OOM result stays in the table as a
secondary signal, with its caveat attached.

## Status

**The floors of 2026-09-20 and 2026-09-23 are void**, and so is the first
attempt at correcting them (the image-layer run of 2026-09-23 17:32, which
this document's middle section is about). The corrected run covers `node`
48/64/96/128, `template` 128/192/256 and `hello` 48/64, all at 1 vCPU, with a
cold cache per cell. Until it lands:

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
