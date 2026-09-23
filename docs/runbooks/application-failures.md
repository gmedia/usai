# The application's own failures

Four things the runtime reports about an application that is running but not
working. None of them is a runtime fault, and none of them is fixed by
restarting the process — which is why they need a page of their own.

## A service gave up

```
WARN service gave up: restart policy exhausted (state failed until the next
     activation; /_usai/ready does not depend on services)
```

Metric: `usai_service{service="…",state="failed"} 1`. Alert on it — this is
the one application state nothing else reports.

**What it means.** A `service()` ran, exited or threw, was restarted
according to its policy, and ran out of restarts. It is not running now, and
nothing will start it again until the **revision is activated again**.

**What to do.**

1. Read why it died: `journalctl -u usai@myapp | grep -E 'service (starting|stopped|failed|gave up)'`
   — each restart logs its reason, and the last one is the cause.
2. Decide whether the work matters right now. Readiness deliberately does
   **not** depend on services: HTTP keeps serving, which is usually right (a
   dead ingest loop should not take the API down) and occasionally wrong. If
   this service is the application's reason to exist, fail the instance
   yourself: your health check can read
   `/_usai/status` → `revisions[].services[].state` and refuse on `failed`.
3. Restart the work without restarting the process: activate the revision
   again through the control surface
   (`POST /revisions/{id}/activate` — `docs/CONTROL-API.md`), or deploy the
   fixed revision. A process restart also works and costs the in-flight
   requests.
4. If it is dying on something transient (a dependency that comes back),
   widen the restart policy rather than removing it: a service with infinite
   restarts and no backoff is a restart storm, below.

## `500 detached_work`

```
WARN http work `POST /invoices` ended with live asynchronous work (1 resource)…
     world=01J… workload="http:POST /invoices" code="detached_work"
```

The line's **message is the explanation**, several sentences of it, and it
changes with the violation; the fact to match on is the structured field
`code="detached_work"` (`"code":"detached_work"` with `--log-format json`,
where the whole paragraph is the `message` value). `grep detached_work`
finds it; `grep 'lifecycle violation'` finds nothing, because no line says
that.

The handler returned while an operation it started was still in flight — a
write, an outbound call — so the runtime cancelled the operation and could
not tell the client whether it happened. It answers `500 detached_work`
**instead of a 200 it cannot stand behind**.

This is an application bug with one shape: a missing `await`. The log line
names the workload; the fix is in the handler.

- `await` the operation, or
- hand it off on purpose: `await ctx.tasks.dispatch(task, input)` (the
  hand-off is awaited, the work is not), or
- publish to a topic if it must survive a crash.

A pending **timer** is different: it is cancelled and the response commits,
with the violation only in the log (and the `x-usai-lifecycle` header in
development).

## `504 deadline_exceeded`

The work passed its deadline: the workload's `timeout:` or the runtime's
default. The world is gone, its operations released, and the client has a
504.

**Before changing the number**, find out which half it is:

- **Waiting** — a slow query, a slow upstream. `usai_http_request_seconds`
  and the resource's own metrics show where the time went. Raising the
  deadline hides it and raises the memory the concurrent worlds hold.
- **Computing** — the CPU slice interrupted a synchronous loop. The
  interpreter is slower than a JIT (GUIDE §18); move the loop to the
  database, or split the work into a task.

A deadline is a promise to the caller, not a limit on the machine: the right
number is the one your client will wait for. Change it per workload
(`timeout: "2s"`), not globally, and only after the log says which half.

## Cron did not run

Cron is scheduled **per instance**. Two things go wrong in opposite
directions:

- **It ran twice.** Two replicas both schedule it. Declare
  `cron(name, { schedule, exclusive: true })` — the tick is claimed in
  PostgreSQL (`usai_cron_ticks`), so exactly one instance runs it — or start
  the others with `--no-cron`.
- **It did not run at all.** The one instance that schedules cron was down
  at that minute, or was started with `--no-cron`. Check
  `usai_cron_ticks_total{state="due"}` per revision and the banner's
  `scheduler` line (`/_usai/status` → `scheduler.cron`).

**There is no backfill.** A tick that nobody ran is gone; the runtime does
not replay schedules. If a missed run must be caught up, run it by hand —
`usai cron run <name>` (one invocation, no wall clock) or `POST /invoke`
through the control surface — and make the handler idempotent for the case
where it runs twice.

## Restart storms

A process that dies and is restarted by systemd or the orchestrator, over and
over, shows as: a rising `usai_build_info` start count, a short
`usai_process_uptime_seconds` every scrape, and the same first error in every
start's log.

Read the **first** error of one start, not the last of the newest — the
interesting line is usually the activation's (a missing environment variable,
a resource that cannot open), and it is the same every time. `usai run`
refuses to start on a missing required variable **by design**, so a restart
loop against an unset variable is the runtime telling you the deployment is
incomplete, not a bug.

While you fix it, stop the loop: `systemctl stop usai@myapp` (or scale the
deployment to zero) so the logs stop rotating away the line you need.
