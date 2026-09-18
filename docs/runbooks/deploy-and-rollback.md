# Bad deployment, rollback, revision replacement

## A broken artifact

`POST /revisions {"artifact": …}` on the control surface answers **422
`invalid_artifact`** with the reason (`this artifact uses manifest format 99
(built with SDK …, runtime …); runtime 0.0.4 understands format 1 only.
Rebuild …`), and nothing else happens: no revision is created, the active
one keeps serving, no log line beyond the request. The same holds for a
signature that does not verify (`artifact refused: …`) when the runtime
requires signatures. `usai run --artifact <bad>` exits 1 with the same
message before listening.

## Replacement without loss

Install (`revision installed`, ~70 ms with a precompiled image), activate
(`revision active`), then drain the previous one (`revision retired`). New
requests route to the new revision from the activation instant; in-flight
ones finish on the old one. The runtime's own test replaces revisions under
load and loses no request; through Caddy the campaign saw a short 5xx blip
(≈1 s) on rollback, which is the proxy's connection reuse across the swap,
not the runtime — a proxy retry policy on connection errors removes it.

## Rollback

Rollback is an install of the previous artifact plus an activation — the
runtime keeps no old revisions once drained. Keep the previous artifact
directory on the host (or the previous image tag). Measured: install +
activate + drain of the replaced revision in 6 s including the drain wait.

## Bounds

The runtime holds at most `max_revisions` (8) revisions across installed,
active and draining; an install past that answers **409
`too_many_revisions`** naming the fix (`DELETE /revisions/<id>` for what
will not be activated). Each held revision costs its compiled image (tens of
MB); the bound exists so an orchestrator bug hits it before the memory
limit does. `GET /revisions` lists them with state and in-flight count.
