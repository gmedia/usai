<!-- A draft for a person to read, edit and post. Not to be filed by a tool:
     bytecodealliance/governance AI_TOOL_POLICY.md asks for a human who is the
     author, has read it, and can answer questions during review. #14399 was
     closed because that was not true of the first attempt.

     Use the "Improvement" template (a feature request); its four headings
     are below. The other three are bug reports and do not fit.

     **Do not hard-wrap the paragraphs when pasting.** GitHub renders a
     single newline inside an issue body as a <br>, unlike a .md file in a
     repository — #14413 went up wrapped at 76 characters and came out with
     21 forced line breaks. Nothing is lost, but the paragraphs read narrow
     on a wide screen and wrap twice on a phone. Put each paragraph on one
     long line; leave the code fence and the bullets alone.

     Historical: this draft became #14413, which #14419 answered (merged
     2026-09-26). One claim in it is wrong and is corrected in
     wasmtime-idle-decommit.md: the kept-resident pages do not hold zeros on
     a slot that has an image, they hold the image. The proposal stands for a
     better reason than the one argued here. -->

**Title:** pooling allocator: a way to release a slot's kept-resident memory when it goes idle

#### Feature

A way for an embedder to release the memory the pooling allocator keeps
resident for slots that are **not in use** (`*_keep_resident`), at a moment
the embedder chooses.

Follow-up from #14357, same embedder: one instance per application, a fresh
instance per request, so the pooling allocator is on the hot path.

#### Benefit

Resident memory follows the **peak** concurrency a process has ever seen
rather than its current load. We measure ~0.5 GiB of resident slots at c=64,
and it does not come back down when the burst ends — a box sized for a daily
peak carries that peak all night.

This is not a leak; it is `linear_memory_keep_resident` doing exactly what it
is for. We set it to 8 MiB because letting those pages go costs about 2× the
CPU per request on one vCPU when the next instance faults them back in. That
is the right trade while slots are reused every few milliseconds, and the
wrong one for a slot nothing has touched for an hour. Today there is no way
to say "keep it while the load lasts".

#### Implementation

Something like:

```rust
impl Engine {
    /// Releases memory the pooling allocator keeps resident for slots that
    /// are not in use. The next instantiation in those slots re-faults the
    /// pages. No effect on other allocators.
    pub fn release_idle_pool_memory(&self) -> usize; // bytes released
}
```

An embedder that already knows when it is idle needs no timer inside
wasmtime — we park our own watchdog when no instance is live, so one call
from that path is enough.

#### Alternatives

- **`PoolingAllocationConfig::decommit_idle_slots_after(Duration)`** — puts
  the policy in wasmtime and needs a timer there. It solves our problem
  equally well; we have no preference and will follow whichever shape you
  prefer.
- **`linear_memory_keep_resident = 0`** — expressible today, and costs the
  ~2× CPU per request above on every request, not just the first after a
  quiet period.
- **Doing it from outside wasmtime** — we looked and could not: the region
  belongs to the pool, there is no handle to an idle slot's memory, and
  `linear_memory_keep_resident` is fixed when the `Engine` is built.

Happy to do the before/after measurement (a burst, then idle, sampling
RSS/PSS) on whichever shape you pick.

<!-- Optional, if you think it helps rather than hurts: a line saying you
     wrote this yourself and can answer questions about it. The policy does
     not require disclosing tool use, and forbids listing a tool as an
     author. -->
