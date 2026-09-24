# ADR-0023: activation retries a dependency, and still fails

**Status:** accepted (v0)
**Date:** 2026-09-24
**Decides:** `docs/OPEN-QUESTIONS.md` → Q20 — whether a bounded activation
retry should be the default.

## Context

`GOAL.md` §32 says activation fails, not the first request: a resource that
cannot be opened stops the process before any listener binds. That contract
is right and this decision does not touch it. A deployment that cannot reach
its database must not start answering as if it could.

What was wrong was the *shape* of the failure on an orchestrator. Exiting
means `CrashLoopBackOff`, whose backoff reaches five minutes — so a pod
restarted for an unrelated reason during a database blip (a node drain, a
scale-up, a spot reclaim, a rollout already in flight) stays down long after
the database is healthy. A 30-second failover becomes a multi-minute outage,
and nothing about it is the database's fault.

`USAI_ACTIVATION_RETRY=<seconds>` has existed since this release and was off
by default, so the capability was there and the default still produced the
outage. A platform engineer evaluating 0.0.9 from the public documents called
it the one change they would require before production.

## Decision

**A dependency is retried for 60 seconds by default**, every 2 s, and then
activation fails with the dependency's own error exactly as before.
`USAI_ACTIVATION_RETRY=0` restores the immediate exit.

**A missing or malformed environment variable is never retried.** No amount
of waiting fixes a configuration error, and failing it at once is what makes
the retry safe to have on: the two failures are distinguishable, and only one
of them is worth waiting for.

Each attempt installs a revision and removes it when it fails, so a minute of
retrying does not exhaust the revision bound and turn a dependency problem
into "too many revisions are held".

## Consequences

- An orchestrator sees a pod that is `Running` and failing its startup probe,
  which is what a startup probe is for, instead of a container that exits.
  Give the startup probe a budget at least as large as the retry.
- A VM or a local run now waits up to a minute before printing the error it
  used to print at once. The WARN line every 2 s says what it is waiting for
  and how to turn it off; `USAI_ACTIVATION_RETRY=0` is the answer for a
  developer who would rather see the failure.
- **Known limitation:** the retry happens before any listener binds, so
  `/_usai/ready` is not answering during it — a probe gets a refused
  connection rather than a 503 naming the resource. That is adequate for a
  startup probe and is the honest description; binding the status listener
  first is a larger change and is not in this decision.
- Like ADR-0022, a contract-shaped default changed once, inside `0.0.x`,
  before the freeze.
