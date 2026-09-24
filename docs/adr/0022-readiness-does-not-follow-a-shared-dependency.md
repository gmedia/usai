# ADR-0022: readiness does not follow a shared dependency

**Status:** accepted (v0)
**Date:** 2026-09-24
**Decides:** `docs/OPEN-QUESTIONS.md` → Q19 — whether `/_usai/ready` should
answer 503 when a bound resource fails its probe.

## Context

`/_usai/ready` exists so a proxy or an orchestrator can take a replica out of
rotation. Until now a bound resource that failed its probe made it answer
503, on the reasoning that a replica which cannot reach its database cannot
serve.

That reasoning holds for exactly one topology: **a replica whose database is
its own.** It takes itself out, the others carry the traffic, and the outage
is contained. That is the case the default was written for.

The common topology is the other one — every replica sharing one PostgreSQL —
and there the coupling inverts its own purpose. Round 16 measured it: direct
to a replica, `hello` answered 200 and `notes` answered 503; through the
proxy block this repository documents, **both** answered 503, because the
proxy had removed the only upstreams there were. One database outage became a
total outage, including every route that never touches the database, and the
routes that could still have served were the ones taken away.

Kubernetes' own guidance says the same thing: do not fail a readiness probe
on a shared dependency, because removing every replica from a Service cannot
help a dependency that is down and does remove the capacity that is up.

It is also not what the runbook trained the operator to expect.
`postgres-down.md` teaches that database-backed routes 503 while the rest
keep serving — which is true of the process and was false of the deployment.

## Decision

**Readiness reports resource health; it does not follow it.** `/_usai/ready`
answers 200 with a bound resource failing its probe, names it in the body,
and the routes that need it answer 503 on their own.

`USAI_READY_REQUIRES_RESOURCES=1` restores the coupling for a deployment
whose replicas each own their database.

Two things do not change:

- **A draining replica always fails readiness.** That is what the rolling
  restart depends on, and it has nothing to do with resources.
- **Activation still fails on a resource that cannot be opened at all**
  (ADR-0023). Readiness is about a running replica, not a starting one.

## Consequences

- A shared-database outage now costs the database-backed routes and nothing
  else, on every replica. The blast radius matches what the runbook says.
- An operator who wants the old behaviour has one variable, and the start-up
  log says which policy is in force — a replica that stayed in rotation
  through an outage should be explicable from its own logs.
- The alert that matters becomes `usai_resource{metric="ready"} == 0` rather
  than readiness. `docs/runbooks/metrics.md` already carries it, and now
  gates the "pool saturated" alert on the resource being ready, so the two do
  not fire together saying different things.
- This is a **contract-shaped default**, changed once, inside `0.0.x`, and
  deliberately before the freeze rather than after it.
