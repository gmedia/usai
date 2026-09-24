# The control surface

`usai run --control <addr>` serves a small JSON-over-HTTP API for whatever
orchestrates the process — a deploy script, systemd's `ExecReload`, Sakala,
the `usai/test` harness — to install, activate, inspect, drain and remove
**revisions** (immutable application definitions) and to stop the runtime.
It is separate from the application listener and from the status listener
(`--status-addr`, which is read-only), and it is not served unless asked
for. `GOAL.md` D15; implementation `crates/usai-runtime/src/control.rs`.

```bash
usai run --artifact .usai/build --port 3000 --control 127.0.0.1:3900
export USAI_CONTROL_TOKEN=<random>        # required when the address is not loopback
```

## Authentication

`Authorization: Bearer <USAI_CONTROL_TOKEN>` on every request. Without a
token configured the surface accepts anything **and refuses to bind to a
non-loopback address** (`refusing to bind the control surface to <addr>
without USAI_CONTROL_TOKEN`). A missing or wrong token is `401
unauthorized`. The token is read at start; rotate by restarting
(`THREAT-MODEL.md`).

## Revisions and states

A revision is one loaded artifact. The runtime holds at most
`max_revisions` (8) at once. States, as `/_usai/status`, the metrics labels and this API spell
them (lowercase; log lines say `revision active`):

| State | Meaning | Next |
|---|---|---|
| `installed` | loaded, resources not yet opened, serving nothing | `activate`, `DELETE` |
| `active` | the one revision that admits new work | `drain`, or replaced by another `activate` |
| `draining` | admits no new work; finishing what it has | retires by itself when in-flight reaches 0 (or at the drain bound); `activate` again is a rollback **while it lasts** — for an HTTP-only service that is milliseconds (see below) |
| `retired` | done; removed from the list right after | — |

Revision ids are `rev<n>` in log lines and `n` (a number) in JSON; the path
segment accepts either (`/revisions/rev5/activate`, `/revisions/5/activate`).

## Endpoints

### `GET /health`

Liveness of the runtime plus the active revision.

```json
{ "ok": true,
  "active": { "id": 5, "identity": "106e99430f4f48d5", "application": "invoicing" },
  "gauges": { "live_worlds": 0, "live_ops": 0, … } }
```

`ok` is false when no revision is active (between a drain and the next
activate). 200 in both cases — it says the *runtime* is up.

### `GET /status`

The full `RuntimeStatus`: the same document as `/_usai/status` on the
status listener (`engine`, `scheduler`, `revisions[]`, `resources[]`,
`worldsInUse`/`worldsMax`, `process`, `http`, …). `docs/runbooks/metrics.md`
describes the fields.

### `GET /revisions`

```json
{ "revisions": [
  { "id": 5, "application": "invoicing", "identity": "106e99430f4f48d5", "state": "active",
    "inFlight": 3, "services": [ … ], "queue": { … }, "cron": { … } } ] }
```

### `POST /revisions` — install

Body `{ "artifact": "<directory>" }`: the path, on the runtime's host, of a
`usai build` output (`manifest.json`, `app.js`, `cache/`, `signature.json`
when signed). The artifact is verified against `--require-signature` when
set, loaded and compiled (or its precompiled image is used), and held as
`installed`. Nothing is served yet, no resource is opened.

- `201 { "id": 6, "identity": "106e99430f4f48d5", "application": "invoicing", "state": "installed", "installed": true }`
- `422 invalid_artifact` — missing files, a bad signature, a manifest the
  runtime cannot execute (the message names the file and the reason).
- `409 too_many_revisions` — `max_revisions` held; drain or `DELETE` one.
- `413 payload_too_large` — the body is bounded at 64 KiB.

**Retrying this call.** By default a second install of the same artifact is
a second revision, holding a second compiled image and counting against the
bound — which is what a control plane's retry-on-timeout produces, with no
way afterwards to tell "my retry landed twice" from "it landed once". Send
`{"artifact": "…", "ifAbsent": true}` and the call becomes idempotent: an
`installed` revision with this artifact's identity is returned instead, with
`"installed": false` so you can tell which happened. Identity is a content
hash, so "the same artifact" means the same bytes, whatever the path.

### `POST /revisions/{id}/activate`

Opens the revision's resources (PostgreSQL pools, caches, HTTP clients —
the same as at `usai run` start: a missing `DATABASE_URL` or an unreachable
database fails here, not at the first request), starts its schedulers and
services, and makes it the one that admits work. The previously active
revision, if any, becomes `draining` and retires by itself when its
in-flight work reaches zero — so an orchestrator that only ever installs
and activates never accumulates revisions.

- `200 { "id": 6, "state": "active", "previous": 5 }` — `previous` is
  `null` when nothing was active.

**Retrying this call is not safe without `expectedPrevious`.** Activating a
`draining` revision again is the rollback (below), and a retry that arrives
while the revision you replaced is still draining is *that same call*: it
will revert whatever deployed in between and answer `200`. Send
`{"expectedPrevious": 5}` — the id you saw active before you started — and
the call is refused with `409 active_revision_moved` when something else
landed under you. A control plane that retries should always send it.

**Activating a different application is refused.** A runtime serving
`invoicing` does not silently become `billing` because a deploy template
interpolated the wrong release directory: that is `409
different_application`. Pass `{"allowApplicationChange": true}` when you
mean it.
- `409 invalid_state` — `revision rev6 is active, already active`: a
  repeat is refused rather than opening the resources and starting the
  schedulers a second time.
- `422 activation_failed` — a resource failed to start (`resource … failed
  to start: cannot connect to …`), a required environment variable is
  missing or malformed (`missing required environment`), or the
  definition is invalid for this runtime. The revision stays `installed`;
  the active one keeps serving.

Activating a `draining` revision again is the **rollback**: it becomes
`active` and the one that replaced it starts draining.

### `POST /revisions/{id}/verify`

Runs **one workload on a revision that is not serving**, against the
resources it will actually use. Same body as `/invoke`:
`{"kind": "task"|"command"|"cron"|"queue", "name": "…", "input": …}` (a
command takes `"args": [...]`).

This is the step that turns a deploy into a check. `activate` validates what
the *manifest* records — a missing `DATABASE_URL`, an unreachable database —
and answers `200` for everything outside it: an unapplied migration, a
schema drift, a resource the developer forgot to list under a workload. By
then the revision you were replacing is already draining away. Verify
first, and nobody's request meets the broken revision.

It prepares the revision as a side effect (resolves its environment, opens
its resources), which is idempotent and is the same work `activate` does —
so a verified revision activates no slower. The revision stays `installed`:
no scheduler runs, no service starts, the listener routes nothing to it.

- `200` — the same body as `/invoke`: `ok`, `value`, `error`, `termination`,
  `durationMs`, `children`, `logs`, `violations`. **`ok: false` is a failed
  verification, not a failed request** — read it and decide.
- `404 unknown_revision` / `404 unknown_workload` — the id, or the name.
- `422` — the revision could not be prepared (a missing variable, a resource
  that will not open); the same failures `activate` reports, found earlier.

Pick a workload that proves the thing you are afraid of: a `task` that
reads the table the release added, a `command` that counts rows. It runs
for real, against the real database — a verification that writes, writes.

### `POST /revisions/{id}/drain`

Marks the revision draining (if it was active, nothing is active
afterwards — new requests are `503 no_active_revision`), asks its services
to stop, waits until in-flight work is zero, then retires and removes it.
Bounded by `--drain-timeout` (30 s).

- `200 { "id": 5, "state": "retired" }`
- `409 would_stop_serving` — it is the **only** revision. Draining it leaves
  nothing to serve and nothing to activate, and the way back is a fresh
  install with an artifact path this process no longer remembers. Install
  the next revision first if you mean to do it.
- `409 invalid_state` — `revision rev5 did not drain within 30s`. **Unlike
  every other `409` on this page, this one is not "nothing happened":** the
  revision stopped admitting work when the call started and stays
  `draining`. Call again, or let the process stop finish it. In-flight work
  is **not** cancelled at the bound here (that is `POST /stop`); it runs to
  completion and its clients get their answers.

The call blocks for as long as the drain takes, up to `--drain-timeout` —
with the 30 s default, a control plane with a 10 s HTTP timeout will time
out on every drain that has work in flight, and there is no handle to poll.

Use this to *stop serving* without stopping the process (a maintenance
window). **You do not need it in a deploy**: `activate` drains the revision
it replaces by itself, and calling `drain` afterwards is how a deployer
turns a working release into a 503.

### `DELETE /revisions/{id}`

Removes an `installed` (never activated) or `retired` revision and returns
its compiled image's memory.

- `200 { "id": 6, "removed": true }`
- `409 invalid_state` — `revision rev5 is active, only an installed or
  retired revision can be removed (drain it first)`.

### `POST /invoke`

Runs one unit of work now, in a fresh world, against the active revision —
what `usai task run`, `usai cron run`, `usai app <command>` and `usai queue
run` do, without a shell on the host:

```json
{ "kind": "task",    "name": "sendInvoice", "input": { "id": 42 } }
{ "kind": "cron",    "name": "nightly" }
{ "kind": "command", "name": "reindex", "args": ["--all"] }
{ "kind": "queue",   "name": "invoice.issued", "input": { "id": 42 } }
```

A queue invocation delivers the message to the consumer directly: a fresh
world, `attempt` 1, no row in `usai_queue`, no retry. Response (200 whether
the handler succeeded or threw — `ok` says which):

```json
{ "ok": true, "value": { … }, "error": null,
  "termination": "completed", "world": 67122, "durationMs": 12,
  "children": [], "logs": [], "violations": [] }
```

`termination` is `completed`, `deadline-exceeded`, `{ "cancelled": { "reason": "…" } }`
or `{ "faulted": { "detail": "…" } }`; on a thrown error `error` carries
`name`, `message` and the `usai` envelope (`code`, `status`, `details`).
`400 invalid_kind` for anything but the four kinds; `409 no_active_revision`
when nothing is active; `404 unknown_workload` (`unknown workload
task:sendInvoic`) for a name the application does not declare. The body is
bounded at 1 MiB.

### `POST /stop`

`202 { "stopping": true }`; the runtime then performs the same shutdown as
SIGTERM (`docs/runbooks/runtime-restart.md`): drain grace, drain, exit 0.

## Errors

Every error is `{ "error": { "code": "<code>", "message": "<text>" } }`:

| Status | Code | When |
|---|---|---|
| 401 | `unauthorized` | token missing or wrong |
| 400 | `invalid_request`, `invalid_kind` | body not the expected JSON |
| 404 | `unknown_revision`, `unknown_route` | no such id (`bad revision id` for a non-numeric segment); no such path |
| 409 | `invalid_state` | the revision is not in a state the operation accepts; the message says which would be |
| 409 | `no_active_revision` | `invoke` while nothing is active |
| 404 | `unknown_workload` | `invoke` names a task, schedule, command or topic the application does not declare |
| 409 | `too_many_revisions` | `max_revisions` held |
| 413 | `payload_too_large` | install body > 64 KiB, invoke body > 1 MiB |
| 422 | `invalid_artifact`, `activation_failed` | the artifact or its activation was refused; the message is the runtime's own diagnostic |
| 403 | `cross_origin_refused` | a **mutating** request carried an `Origin` header. The control surface is not a browser API, and a bodyless `POST` is a CORS simple request: reachable from any page the operator has open. `GET /health`, `/status` and `/revisions` still answer with an `Origin` present, so a proxy that adds the header does not break a health check |
| 415 | `unsupported_media_type` | a mutating request's `Content-Type` was `application/x-www-form-urlencoded`, `multipart/form-data` or `text/plain` — a browser's simple set. Use `application/json`, or send no body |
| 500 | `runtime_error` | anything else — report it |

## A deploy, end to end

```bash
C=http://127.0.0.1:3900; H="Authorization: Bearer $USAI_CONTROL_TOKEN"
usai db migrate --artifact /srv/app/releases/$sha                  # once per release
was=$(curl -sf -H "$H" $C/health | jq -r '.active.id // "null"')     # what you are replacing
id=$(curl -sf -H "$H" -H 'content-type: application/json' -X POST $C/revisions \
       -d "{\"artifact\":\"/srv/app/releases/$sha\",\"ifAbsent\":true}" | jq .id)
# Exercise it before it takes traffic: the failures `activate` cannot see are
# the ones outside the manifest (an unapplied migration, a schema drift).
curl -sf -H "$H" -H 'content-type: application/json' -X POST $C/revisions/$id/verify \
       -d '{"kind":"task","name":"smoke"}' | jq -e '.ok' || exit 1
curl -sf -H "$H" -H 'content-type: application/json' -X POST $C/revisions/$id/activate \
       -d "{\"expectedPrevious\":$was}"                                # old revision drains itself
# The gate is yours: readiness is 200 before, during and after both a healthy
# and a broken activation, because it answers "is *a* revision serving". Send
# real requests, or difference usai_http_responses_total{class="5xx"} over a
# window, and roll back on what you see.
# rollback: install the previous artifact again and activate it. Re-activating
# the revision you just replaced works only while something long-running still
# holds it draining; keep the previous artifact directory mounted.
```

**The rollback handle is the previous artifact directory, not the previous
revision.** A replaced revision drains and then stops being listed — measured
at under 100 ms for a revision with nothing in flight, which is every
HTTP-only service between requests — and `activate` on it then answers `404
unknown_revision`. With an eight-second export still running it stayed
`draining` and `activate` returned `200`, so the handle is real; it is just
not one to plan a rollback around. `POST /revisions` with `ifAbsent: true` on
the previous directory, then `activate`, always works — which is why the
directory has to still be there.

**Revisions live in the process, and `--artifact` is what a restart serves.**
The instant your first control-plane deploy succeeds, the running revision
and the `--artifact` the process was started with disagree — and a crash, an
OOM kill, `systemctl restart`, a host reboot or the documented way to rotate
`USAI_CONTROL_TOKEN` (restart it) all bring back whatever `--artifact`
points at. Ids restart at 1 and are reused, so an id your control plane
still holds may name a different artifact afterwards. **Rewrite the path
`--artifact` resolves** — the `current` symlink in `systemd.md`'s layout —
as part of every deploy, or your next restart is an unannounced rollback.

The process never restarts, the listener never closes, no request is
refused: the replaced revision finishes its in-flight work while the new
one admits. What this does **not** do is upgrade the runtime binary — that
is a process restart (`docs/runbooks/systemd.md`).
