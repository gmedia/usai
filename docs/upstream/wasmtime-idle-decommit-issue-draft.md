<!-- A draft for a person to read, edit and post. Not to be filed by a tool:
     bytecodealliance/governance AI_TOOL_POLICY.md asks for a human who is the
     author, has read it, and can answer questions during review. #14399 was
     closed because that was not true of the first attempt. -->

**Title:** pooling allocator: a way to release a slot's kept-resident memory when it goes idle

Follow-up from #14357. Same embedder: one instance per application, a fresh
instance per request, so the pooling allocator is on the hot path.

We set `linear_memory_keep_resident` to 8 MiB because letting those pages go
costs about 2× the CPU per request on one vCPU when the next instance faults
them back in. That is the right trade while slots are reused every few
milliseconds. It is the wrong one for a slot nothing has touched for an hour:
resident memory follows the **peak** concurrency the process has ever seen,
not the current load, so a box sized for a daily peak carries that peak all
night. We measure ~0.5 GiB of resident slots at c=64 and it does not come
back down.

We cannot do this from outside. The kept-resident region belongs to the pool
and there is no handle to an idle slot's memory, and
`linear_memory_keep_resident` is fixed when the `Engine` is built — so an
embedder can say "always keep 8 MiB" or "never keep any", but not "keep it
while the load lasts".

Would you take a way for the embedder to release it? We already know when we
are idle, so we do not need a timer inside wasmtime — something like
`Engine::release_idle_pool_memory()` would be enough. A
`decommit_idle_slots_after(Duration)` on `PoolingAllocationConfig` would also
solve it; we have no preference and will follow whichever shape you prefer.

Happy to do the before/after measurement (a burst, then idle, sampling
RSS/PSS) on whichever one you pick.

<!-- Optional, if you think it helps rather than hurts: a line saying you
     wrote this yourself and can answer questions about it. The policy does
     not require disclosing tool use, and forbids listing a tool as an
     author. -->
