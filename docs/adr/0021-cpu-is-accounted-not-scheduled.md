# ADR-0021: CPU is accounted, not scheduled

**Status:** accepted (v0)
**Date:** 2026-09-23
**Decides:** the gap `docs/THREAT-MODEL.md` has carried since it was written
("CPU is accounted, not scheduled … there is no per-workload CPU share or
fairness policy yet") and `docs/ACCEPTANCE-AUDIT.md` lists as an open item.

## Context

One runtime is one trust domain (ADR-0008). Inside it, every world's guest
execution **blocks the worker thread it runs on** — that is what makes a
fresh world cheap and what makes the ownership ledger simple. The
consequence is that a workload which spends CPU takes it from the others:
an export that loops over a million rows in the interpreter, a handler doing
Argon2 in a request, a cron tick that re-renders everything nightly.

What already bounds it:

- **The deadline.** Finite work has one (per workload, or the runtime's
  default), and it ends the world whether the work was waiting or computing.
- **The CPU slice.** One synchronous run is interrupted at 5 s, so a loop
  that never awaits cannot hold a thread forever.
- **Admission.** `concurrency:` on a workload caps how many of its worlds
  exist at once, so a heavy workload can be given a lane narrower than the
  runtime's.
- **The cgroup.** A container's CPU limit is the outer bound, and P8E
  measured the envelope against it.

What none of them give is a *share*: nothing says "reports may have 20 % of
this process". Two workloads at the same concurrency get whatever the
scheduler happens to give them.

## Decision

**v0 accounts CPU per workload and does not schedule it.** Concretely:

1. Every world's guest CPU time is measured (thread CPU inside guest
   entries) and attributed to its workload:
   `usai_workload_cpu_seconds_total{workload="…"}` and, in
   `/_usai/status`, `gauges.guestCpuNsByWorkload`. The map is bounded by the
   workloads a definition names — never by request data.
2. The tools an operator has are **admission and the deadline**, and they
   are enough for the shape the problem actually takes: one known-heavy
   workload beside many light ones. Give the heavy one a `concurrency:` it
   cannot exceed and a deadline it cannot outlive; the accounting says
   whether the number was right.
3. **No scheduler, no quota, no priority** in v0.

## Why not a scheduler

- Fair scheduling means preempting or queueing guest execution, and guest
  execution is a blocking call into an interpreter. Preemption would mean an
  interruption point the guest can be resumed from — which is the same
  machinery a JIT would need, and it makes state survive a world (Q18's
  problem, ADR-0019's reason for not going there).
- Queueing by share, rather than preempting, is a policy that has to be
  configured, and configuring it needs the number the accounting now
  provides. Building the policy before anyone has read the number would be
  guessing at both.
- Within one trust domain, a workload that starves the others is an
  application bug with an owner. Across trust domains it is a security
  property — and untrusted co-tenancy already needs a new ADR and a new
  threat model (ADR-0008, Q17). This decision does not extend to that case
  and must not be read as covering it.

## Consequences

- `docs/THREAT-MODEL.md`'s line stays true and now points here, with the
  metric that makes it actionable: **you can see which workload spends the
  CPU, and you cap it with admission.**
- An operator's recipe, in `docs/runbooks/overload.md`'s terms: watch
  `usai_workload_cpu_seconds_total`, find the workload whose share does not
  match its importance, give it a `concurrency:` and a deadline, and watch
  the number again.
- **Revisit when** a real deployment shows a starvation the two tools cannot
  express — the record to bring is the per-workload CPU series, not an
  anecdote — or when untrusted co-tenancy is on the table, which changes the
  question from fairness to isolation.
