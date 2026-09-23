# A token or a key is compromised

`docs/THREAT-MODEL.md` names the scenarios; this is what to do about them.
Four secrets exist around a deployment, and only one of them is fixed by a
restart.

| Secret | What it grants | Rotation |
|---|---|---|
| `USAI_STATUS_TOKEN` | reads `/_usai/status`, `/_usai/metrics`, `openapi.json`: a traffic map, environment *names*, cron schedules, queue schemas | restart with a new value |
| `USAI_CONTROL_TOKEN` | **installs, activates, drains and removes revisions, and stops the process** — the deployment itself | restart with a new value, then audit (below) |
| The artifact signing key (`usai keygen`) | makes an artifact that a runtime started with `--require-signature <that public key>` will run | **a restart does not help** — see below |
| `DATABASE_URL` | the data, and the queue table | the database's own rotation, then restart |

## First, decide whether it was used

The runtime does not hide any of this, but it also does not alert on it:

```bash
# Every revision the process ever loaded, in order, with what it was.
journalctl -u usai@myapp | grep -E "revision installed|revision active|revision retired"
# Refusals on the control surface (a wrong or missing token).
journalctl -u usai@myapp | grep '"code":"unauthorized"'
# What is loaded right now, and what it hashes to.
curl -s -H "Authorization: Bearer $USAI_CONTROL_TOKEN" 127.0.0.1:3900/revisions | jq
```

`revision installed` carries `application` and `identity` (the artifact's
content hash). **Compare every identity in the log against the identities
your pipeline built.** An identity you cannot account for is an install you
did not make, and everything after it ran that code.

The control surface logs a refusal without the presented value, so a burst of
`unauthorized` is an attempt, not a leak; an install you did not make is the
leak.

## `USAI_STATUS_TOKEN` or `USAI_CONTROL_TOKEN`

1. Generate a new value (`openssl rand -hex 32`) and put it where the process
   reads it (systemd's `EnvironmentFile`, the orchestrator's secret).
2. Restart — a rolling restart behind a proxy costs nothing
   (`deploy-and-rollback.md`). The tokens are read at start; there is no
   reload.
3. For the control token, also **audit what it could have done while it was
   valid** with the commands above, and confirm the active revision's
   identity is one you built.
4. If the surface was reachable from outside the host, close that too: the
   control listener refuses a non-loopback address without a token, but with
   a token it will bind anywhere you tell it to. It belongs on loopback or a
   private interface, behind the orchestrator — never on the application's
   listener.

## The signing key

This is the one where rotation is not the fix. Anything signed with that key
is accepted by every runtime configured to trust it, including artifacts made
*after* the key leaked and *before* you noticed.

1. **Stop trusting the key.** Change `--require-signature` /
   `USAI_REQUIRE_SIGNATURE` on every instance to the new public key only, and
   restart. Until that is done, a new key does not help — the old one is
   still accepted.
2. **Re-sign what you still trust.** Build the release from source again,
   sign it with the new key (`usai keygen`, `usai build --sign`), and deploy
   that. Do not re-sign an artifact directory you already have unless you can
   account for how it got there.
3. **Audit the installs** as above. `identity` is the artifact's content
   hash: an artifact signed by the leaked key still has to *be* something,
   and the hash tells you whether it is your build.
4. **Then rotate everything the compromised artifact could have read** — the
   database URL, any API credential in the environment. A signed artifact
   runs as the application, so its capabilities are the application's.

If artifacts are fetched from a registry or an orchestrator rather than built
on the host, treat the registry's copy as suspect too: delete the tags the
leaked key signed rather than leaving them for a rollback to find.

## Prevention, in the order that pays

- Keep the control surface on loopback (`--control 127.0.0.1:3900`) and reach
  it through the orchestrator, not the network.
- Give the status surface its own listener (`--status-addr`) and a token, or
  switch off what nobody consumes (`USAI_SURFACES_OFF`).
- Keep the signing key off the machines that run the application: sign in the
  build, ship the artifact, and let the runtime only verify.
- Set the tokens through a secret manager rather than a file that a backup
  will copy.
- `scripts/qualification/threat/run.sh` checks the enforcement side of all of
  this (a wrong token is 401, a tampered artifact is refused, another key's
  signature is refused) — run it after changing any of it.

## What this runtime does not do for you

There is no revocation list, no key expiry and no audit log beyond the
process's own. The identity in `revision installed` is the audit trail, and
it is only as useful as your record of what your pipeline built — keep that
record.
