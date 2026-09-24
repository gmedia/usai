# Releasing a pooling slot's kept-resident memory when it goes idle

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

The kept-resident region is **anonymous memory the pool `memset`s to zero** on
deallocation — `memory_pool.rs`: *"This much memory will be `memset` to zero
when a linear memory is deallocated. Memory exceeding this amount … will be
released with `madvise`"*. So the pages hold zeros, not the module's image;
the image arrives through the copy-on-write mapping when the next instance
starts.

That makes the *content* question trivial: `MADV_DONTNEED` over that region
gives back zero-filled pages on the next touch, which is exactly what is
there now. Releasing it is a pure memory-for-page-faults trade with no
correctness consequence.

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

## What to measure before and after

The same cells as P8E, on the qualification VM: a c=32 burst, then 300 s idle,
sampling RSS/PSS — today the curve is flat after the burst, and the change
should bring it back toward the pre-burst plateau at the cost of the first
request after the quiet period. The P8E harness already produces exactly this
shape (`scripts/qualification/p8e/fleet.sh floor …`), so the before/after is a
re-run, not new tooling.

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
