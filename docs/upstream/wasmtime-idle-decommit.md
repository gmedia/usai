# Releasing a pooling slot's kept-resident memory when it goes idle

Status (2026-09-26): **merged upstream, built here, and measured.**
[bytecodealliance/wasmtime#14419](https://github.com/bytecodealliance/wasmtime/pull/14419)
was merged into `main` on 2026-09-26 (`65c5190`) by cfallin, closing
[#14413](https://github.com/bytecodealliance/wasmtime/issues/14413).
`Engine::release_idle_pool_memory()` exists upstream; review was one approval
plus four comments, all of them about the comments rather than the code.

**Built here in 0.0.13, and then measured — and the measurement amended the
lever.** The merged diff applies cleanly to the vendored 48.0.2, so this did
not wait for a wasmtime release (`vendor/README.md`, second patch); the call
site is the epoch ticker, which is the one thread that already knows the
process has gone quiet.

What the before/after found is **not** what this document predicted
(`docs/measurements/2026-09-26-idle-decommit.md`, VM 47): a c=32
contract-heavy burst leaves **12 MiB** of accounted warm-unused bytes against
**66 MiB** of post-burst RSS growth, and releasing those 12 MiB does not move
RSS at all — measured twice. The per-concurrency cost is real
(`USAI_WASM_KEEP_RESIDENT=0` takes the same burst from 111 to 50 MiB, at
≈16 % throughput), but it is not a warm-slot plateau:

- `max_unused_warm_slots(0)`, which this runtime sets because the P6 churn
  campaign OOM'd without it, re-picks freed slots instead of accumulating
  them — so it had **already closed** the plateau this lever targets, a week
  before the lever was written down;
- what remains is per-slot copy-on-write image pages (RSS 111 against PSS
  63), which `keep_resident` holds by resetting them in place and which the
  allocator does not count in the per-slot `bytes_resident` this API reads.

So the implementation ships **off by default** (`USAI_IDLE_DECOMMIT_MS`), with
the plateau gauge on, because how small the plateau is next to RSS is the
finding worth surfacing. Nothing about the upstream change is wrong — the
256 MiB it gives back for a default-tuned embedder is in
`idle-decommit-measurement/` and stands. What was wrong was assuming we were
that embedder.

The vendor still goes away when a release carries both patches, by the file
rather than the date:

```bash
ref=v50.0.0   # whichever release you are considering
gh api "repos/bytecodealliance/wasmtime/contents/crates/wasmtime/src/engine.rs?ref=$ref" \
  -q .content | base64 -d | grep -c release_idle_pool_memory
```

---

The history below is kept because the first attempt's closure is the part
worth remembering.

Status (2026-09-25): **open as
[bytecodealliance/wasmtime#14413](https://github.com/bytecodealliance/wasmtime/issues/14413)**,
filed by the maintainer of this repository under wasmtime's *Improvement*
template — the four headings it answers are theirs. It is the second
attempt; the first was
[#14399](https://github.com/bytecodealliance/wasmtime/issues/14399),
closed the same day by a maintainer under the Bytecode Alliance
[AI tool policy](https://github.com/bytecodealliance/governance/blob/main/AI_TOOL_POLICY.md):
*"This issue doesn't appear to comply with our policy on tool-generated
content, and requires additional justification for why it is valuable enough
to the project for us to read it… please re-open as a new issue."*

**That call was right, and the fault is on our side of the fence.** The
policy's requirement is a human in the loop who has read the text, is its
author, and can answer questions about it during review; it explicitly names
agents acting in the project's spaces without human approval. The text below
was drafted by an assistant and filed without the maintainer of this
repository reading it, which is the pattern the policy exists to stop. The
same week's #14357 was merged within a day — a small change, with tests,
whose value was obvious against the time it cost to review. The difference
is the policy's golden rule, not the subject matter.

**What the second attempt did differently**: a person read it and judged it
worth a maintainer's time before it was posted, and posted it themselves. It
is also less than half the length, fitted to wasmtime's own template, and
asks a question ("would you take a way for the embedder to release it?")
rather than presenting a design. The draft it came from is
`wasmtime-idle-decommit-issue-draft.md` beside this file.

**If it is declined again**, that is their call about their own review time
and nothing here depends on it: this is P9 scope. What we owe the thread if
it is taken up is the before/after measurement below.

The original framing follows, as a follow-up from the same embedder as the
merged #14357. This is the second
of the three levers in `docs/adr/0019-p9-scope-the-runtime-tax-we-keep.md`,
and the investigation below is why it belongs upstream rather than in this
repository.

The issue is this document with the parts addressed to *this* repository's
readers removed, the measurements restated as an embedder's rather than by
internal filename, and one line saying the numbers come from a process that
runs one instance per application. What we owe the thread now is the
before/after measurement named below, on whichever shape the maintainers
prefer.

## What an operator sees today

Resident memory follows the **peak** concurrency the process has ever seen,
not the current load. The concurrency sweep (`2026-09-23-sweep.md`) measured
≈0.5 GiB of resident slots at c=64 against a single Node process's 143 MiB,
and P8E (`2026-09-20-p8e-efficiency.md`) measured ≈4 MiB RSS / ≈1.5 MiB PSS
per slot ever touched. After the burst ends the memory stays: a box sized for
a daily peak carries that peak all night.

This is not a leak — it is the documented behaviour of
`PoolingAllocationConfig::linear_memory_keep_resident`, which this runtime
sets to 8 MiB (`WasmConfig::linear_memory_keep_resident`) because faulting
those pages back in costs about twice the CPU per request on one vCPU
(measured, P8E's C2 cells, `USAI_WASM_KEEP_RESIDENT=0`). The trade is right
while a slot is being reused every few milliseconds. It is wrong for a slot
nothing has touched for an hour.

## Why the embedder cannot do it

> **Correction (2026-09-26).** The issue text and an earlier version of this
> section said the kept-resident pages hold zeros. That is wrong on Linux for
> a slot that has an image: the reset writes the image back into the region,
> so the pages hold the image, not zeros. The conclusion survives anyway, for
> a different reason than the one given — see below.

The kept-resident region is the part of a freed slot the pool resets **in
place** rather than releasing — `memory_pool.rs`: *"This much memory will be
`memset` to zero when a linear memory is deallocated. Memory exceeding this
amount … will be released with `madvise`"*. On a platform whose
`decommit_behavior` is `RestoreOriginalMapping`, that in-place reset restores
the slot's original contents: the module's image where there is one, zeros
where there is not.

That is what makes the *content* question trivial, and it is a stronger
guarantee than "the pages are zeros": `MADV_DONTNEED` on a private file
mapping restores exactly those same original contents on the next touch. So
the resident region holds precisely what the mapping would give back for
free, and releasing it is a pure memory-for-page-faults trade with no
correctness consequence. The merged test checks this directly — it reads the
module's data back out of a slot whose resident pages were released.

What the embedder does not have is the **address**. Slot memory belongs to the
pool; `wasmtime` exposes no handle to an idle slot's region, and reaching into
the mapping from outside the allocator would be writing against private
bookkeeping (`ImageSlot`) even when the bytes agree.

Nor can the existing knob do it: `linear_memory_keep_resident` is fixed when
the `Engine` is built, so an embedder can choose "always keep 8 MiB" or "never
keep any", but not "keep it while the load lasts".

## The proposal

Add an explicit, embedder-driven release. This runtime already knows when it
is idle — `crates/usai-runtime/src/idle.rs` parks the watchdog and the epoch
ticker when no world is live — so it needs no timer inside Wasmtime, only a
way to act on what it already knows:

```rust
impl Engine {
    /// Releases memory the pooling allocator is keeping resident for slots
    /// that are not in use (`*_keep_resident`). The next instantiation in
    /// those slots re-faults the pages. No effect on other allocators.
    pub fn release_idle_pool_memory(&self) -> usize; // bytes released
}
```

An alternative shape — `PoolingAllocationConfig::decommit_idle_slots_after(Duration)`
— puts the policy in Wasmtime and needs a timer there; the explicit call is
smaller, testable, and leaves the policy where the knowledge is. Either way
the embedder's side here is one call from the idle path plus a
`usai_pool_memory_released_total` counter.

## Measured

Done 2026-09-25, against the PR, with a standalone program kept beside this
file (`idle-decommit-measurement/`): 64 slots at `keep_resident` 8 MiB give
back **256 MiB** of RSS (266.9 → 10.9 MiB), the control holds it, and the
first instantiation afterwards pays **~2 ms** re-faulting before the slot is
warm again. The full table is in that directory's README.

## What to measure before and after — done, 2026-09-26

The plan here was: a c=32 burst on the qualification VM, then idle, sampling
RSS/PSS, expecting the curve to come back toward the pre-burst plateau at the
cost of the first request afterwards. It was run
(`docs/measurements/2026-09-26-idle-decommit.md`) and **the curve did not come
back**: the release fires, gives back the 12 MiB it accounts for, and RSS is
unchanged. The prediction in this paragraph was wrong, and the reason is in the
status block at the top — worth leaving here rather than editing away, because
the shape of the mistake (a lever written from an RSS number without checking
what that RSS was made of) is the reusable part.

## Why not now, locally

Because the honest local version is a hack (madvise behind the allocator's
back) and the honest upstream version is ~30 lines in a file this project has
already contributed to — `bytecodealliance/wasmtime#14357` was merged on
2026-09-23. Opening the second PR is cheap; shipping a private patch for it
would re-create the vendor we are about to delete.

## How it was opened, and what that cost

The decision to open it was the maintainer's, taken 2026-09-24; the *drafting
and filing* were not, and that is what the closure was about. A second
attempt has to be written and posted by a person. This document **is** the
issue — it states the observation, the measurement, why
the embedder cannot do it, and the shape of the change:

```bash
gh issue create --repo bytecodealliance/wasmtime \
  --title "pooling allocator: release a slot's kept-resident memory when it goes idle" \
  --body-file docs/upstream/wasmtime-idle-decommit.md
```

Trim the first paragraph (it is addressed to this repository's readers) and
say in one line that the numbers come from an embedder running one instance
per application. Link the merged #14357 as context for who is asking.
