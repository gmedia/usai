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
| `draining` | admits no new work; finishing what it has | retires by itself when in-flight reaches 0 (or at the drain bound); `activate` again = rollback |
| `retired` | done; removed from the list right after | — |

Revision ids are `rev<n>` in log lines and `n` (a number) in JSON; the path
segment accepts either (`/revisions/rev5/activate`, `/revisions/5/activate`).

## Endpoints

### `GET /health`

Liveness of the runtime plus the active revision.

```json
{ "ok": true,
  "active": { "id": 5, "identity": "sha256:…", "application": "invoicing" },
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
  { "id": 5, "application": "invoicing", "identity": "sha256:…", "state": "active",
    "inFlight": 3, "services": [ … ], "queue": { … }, "cron": { … } } ] }
```

### `POST /revisions` — install

Body `{ "artifact": "<directory>" }`: the path, on the runtime's host, of a
`usai build` output (`manifest.json`, `app.js`, `cache/`, `signature.json`
when signed). The artifact is verified against `--require-signature` when
set, loaded and compiled (or its precompiled image is used), and held as
`installed`. Nothing is served yet, no resource is opened.

- `201 { "id": 6, "identity": "sha256:…", "application": "invoicing", "state": "installed" }`
- `422 invalid_artifact` — missing files, a bad signature, a manifest the
  runtime cannot execute (the message names the file and the reason).
- `409 too_many_revisions` — `max_revisions` held; drain or `DELETE` one.
- `413 payload_too_large` — the body is bounded at 64 KiB.

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

### `POST /revisions/{id}/drain`

Marks the revision draining (if it was active, nothing is active
afterwards — new requests are `503 no_active_revision`), asks its services
to stop, waits until in-flight work is zero, then retires and removes it.
Bounded by `--drain-timeout` (30 s).

- `200 { "id": 5, "state": "retired" }`
- `409 invalid_state` — `revision rev5 did not drain within 30s`: the
  revision stays `draining`; call again, or let the process stop finish it.

Use this to *stop serving* without stopping the process (a maintenance
window). To replace, `activate` the new one instead; the old drains on its
own.

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
  "termination": "completed", "world": "w-…", "durationMs": 12 }
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
| 500 | `runtime_error` | anything else — report it |

## A deploy, end to end

```bash
C=http://127.0.0.1:3900; H="Authorization: Bearer $USAI_CONTROL_TOKEN"
usai db migrate --artifact /srv/app/releases/$sha                  # once per release
id=$(curl -sf -H "$H" -X POST $C/revisions -d "{\"artifact\":\"/srv/app/releases/$sha\"}" | jq .id)
curl -sf -H "$H" -X POST $C/revisions/$id/activate                 # old revision drains itself
until curl -sf http://127.0.0.1:9090/_usai/ready >/dev/null; do sleep 0.5; done
# rollback = activate the previous id while it is still draining (or install it again)
```

The process never restarts, the listener never closes, no request is
refused: the replaced revision finishes its in-flight work while the new
one admits. What this does **not** do is upgrade the runtime binary — that
is a process restart (`docs/runbooks/systemd.md`).
