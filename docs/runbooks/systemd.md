# Running on a VM under systemd

The compose file (`docs/deploy/compose.production.yaml`) is one shape; a
plain VM with the native binary and systemd is the other, and everything the
runtime does for a stop, a rolling restart and a rollback is the same — the
orchestrator's knobs just have other names. Nothing here is Usai-specific
beyond the flags; it is written down because it is how many teams run
things.

## Layout

```text
/usr/local/bin/usai                      the release binary (SHA-256 verified from the GitHub release)
/srv/app/releases/<version-or-sha>/      one artifact per release: `usai build` output (manifest.json, app.js, cache/, migrations/, signature.json)
/srv/app/current -> releases/<sha>       the artifact the unit serves (a symlink; rollback = repoint + restart)
/etc/usai/app.env                        DATABASE_URL and the rest of the declared environment (root:usai 0640) — `usai run` never reads .env
```

Build the artifact where the source is (CI, a build host), copy the directory
to the VM, run the migrations from the VM once per release (`usai db migrate
--artifact /srv/app/releases/<sha>`; several jobs at once serialize on an
advisory lock), then switch the symlink and restart.

## Unit

A **template unit**: `usai-app@1` and `usai-app@2` are two instances of one
file, each with its own port derived from the instance name (`%i`). One
instance is `systemctl enable --now usai-app@1`; a second replica on the
same VM is `usai-app@2`.

```ini
# /etc/systemd/system/usai-app@.service
[Unit]
Description=Usai application (app, replica %i)
After=network-online.target postgresql.service
Wants=network-online.target

[Service]
User=usai
Group=usai
EnvironmentFile=/etc/usai/app.env
# Per-replica overrides (USAI_NO_CRON=1 on every replica but one for
# schedules that are not `exclusive`), absent when the file does not exist.
EnvironmentFile=-/etc/usai/app-%i.env
Environment=USAI_MAX_WORLDS=48
# Zero-502 rolling restart: readiness fails and connections close after
# their response for the grace, then in-flight work drains (bounded).
Environment=USAI_DRAIN_GRACE=2 USAI_DRAIN_TIMEOUT=30
# The private surfaces on an address only the proxy and the scraper reach.
# One loopback address per replica, same ports on each: a proxy's health
# check is usually one port for the whole upstream group, so per-replica
# *ports* cannot be expressed in it — `deploy-and-rollback.md` → The proxy
# configuration has the block and what goes wrong without this.
Environment=USAI_STATUS_ADDR=127.0.0.%i:9090 USAI_STATUS_TOKEN=<random>
# The application on 127.0.0.1:3001, 127.0.0.2:3001, …
# `--log-format json` writes the log as JSON on stderr and leaves stdout
# empty (the startup banner becomes one `serving` line), so a shipper that
# reads the journal parses every line it sees.
ExecStart=/usr/local/bin/usai --log-format json run --artifact /srv/app/current --host 127.0.0.%i --port 3001 --require-signature <public-key-hex>
# SIGTERM is the drain signal; give it grace + drain + a margin before SIGKILL.
KillSignal=SIGTERM
TimeoutStopSec=40
Restart=on-failure
RestartSec=1
# The runtime needs a writable /tmp for nothing but its own scratch; the
# artifact is read-only.
ProtectSystem=strict
ReadOnlyPaths=/srv/app
PrivateTmp=yes
NoNewPrivileges=yes
LimitNOFILE=65536
MemoryMax=512M
# Do NOT add LimitAS (or `ulimit -v`). The world pool reserves address space
# per slot up front — ~200 GB of virtual mapping for 48 worlds, a few tens of
# MB of it resident — so any address-space cap kills the process on the first
# world. `MemoryMax` is the correct limit: it bounds what is actually used.

[Install]
WantedBy=multi-user.target
```

```bash
# /etc/usai/app-2.env — the second replica does not schedule cron
USAI_NO_CRON=1
```

`MemoryMax` is the same number as the compose `mem_limit` (`SUPPORTED.md` →
Host envelope: 192 MiB is the supported floor for 48 worlds; 512 M leaves
room for a held revision and a burst; add ≈150–200 MiB when the application
hashes passwords under load — `sizing.md` → Per-workload memory).
`TimeoutStopSec` must exceed `USAI_DRAIN_GRACE + USAI_DRAIN_TIMEOUT`; the
compose file uses 35 s for 2 + 30.

Exactly one replica runs the scheduler for schedules that are not
`exclusive: true` — the others get `USAI_NO_CRON=1` in their `app-%i.env`
(and `USAI_NO_SERVICES=1` when a `service()` must be single). The proxy in
front health-checks each replica's `/_usai/ready` on its own address
(`127.0.0.1:9090`, `127.0.0.2:9090`) and retries a refused connection on
another (`deploy-and-rollback.md` → The proxy configuration has the block,
in full, and the mistake it exists to prevent; the health interval must be
at or below the drain grace).

Readiness is also how a database outage reaches your users: an unready
replica is removed from rotation, so a shared PostgreSQL outage takes out
every route on every replica, not only the ones that query it. That default
and the knob that changes it are in `postgres-down.md`.

A single-replica VM is the same unit as `usai-app@1` — there is no
non-template variant to maintain.

## Deploy (rolling)

```bash
sha=<new>
rsync -a build/ /srv/app/releases/$sha/
sudo -u usai usai db migrate --artifact /srv/app/releases/$sha        # once per release, any replica
ln -sfn releases/$sha /srv/app/current
for unit in usai-app@2 usai-app@1; do                                 # one at a time
  systemctl restart $unit
  until curl -sf http://127.0.0.${unit#usai-app@}:9090/_usai/ready >/dev/null; do sleep 0.5; done
done
```

What the restart does, in the log (`journalctl -u usai-app@1 -o cat`):
`SIGTERM received` → `shutting down: readiness now fails …` (the proxy stops
routing here within its health interval) → `shutting down: draining in-flight
work` → `revision retired`, `http listener closed; draining connections` →
`drained; ownership returned to baseline` → exit 0 → systemd starts the new
process → `revision active` → `/_usai/ready` 200. Measured with two replicas
behind Caddy: 0 × 502 over 40 536 requests during two restarts
(`docs/measurements/2026-09-18-p5-p6-qualification.md` → Two replicas).

A restart of a **single** replica costs the seconds between exit and the new
process's first listen (2–3 s measured); there is no way around that with one
process — run two.

## Rollback

```bash
ln -sfn releases/<previous> /srv/app/current
systemctl restart usai-app@2 && systemctl restart usai-app@1   # same rolling loop
```

The previous artifact still exists (never delete the last two), the runtime
that served it is the same binary, and migrations are immutable once applied
— write them to be compatible with the previous code (`SUPPORTED.md` →
Versioning). If the new artifact fails to start (`missing required
environment`, `artifact refused`), the unit exits 1 and `Restart=on-failure`
retries every second: the log names the cause (`invalid-config.md`); repoint
the symlink and restart.

## Upgrading the runtime binary

Runtime and SDK ship together; an artifact built by version N runs on the
runtime of N and N+1 (`SUPPORTED.md`). Order: **install the new binary**
first, then — if the release notes mention new queue indexes — run
`usai --root /srv/app queue prepare` with it while the old version keeps
serving (the subcommand only exists on the version that needs it, so this
step is impossible before the binary is installed), then **restart the
replicas one at a time** (they now run the old artifact on the new runtime),
then **deploy the artifact** built with the new SDK the same way. The other
order — artifact first — is the common mistake, and it serves silently
whenever the manifest format did not change; since 0.0.10 the runtime warns
when the artifact was built by a newer SDK than itself. A
0.0.5 and a 0.0.6 runtime can serve behind one proxy during the roll
(measured: `docs/measurements/2026-09-18-p5-p6-qualification.md` → Mixed
versions); they share nothing but PostgreSQL. The migration ledger is stable
across versions; **`usai_queue` is brought up to date by the first process of
the new version** — in 0.0.6 a column and two indexes, and the indexes build
under a lock that blocks publishes and claims. On a queue table that was
never pruned that lock is the outage: build them first, while the old
version keeps serving, with

```bash
usai --root /srv/app queue prepare        # CREATE INDEX CONCURRENTLY, repeatable
usai --root /srv/app queue status         # rows per topic and state, oldest wait
usai --root /srv/app queue prune --state done --older-than 30d --yes   # the table is yours
```

`prepare` is safe to run at any time and as often as you like; the runtime's
own `CREATE INDEX IF NOT EXISTS` then finds the index already there and takes
no lock. **Nothing prunes by itself** — a `dead` row is a message your
application failed, and only you know whether it is still evidence.
Rolling the binary back needs no database change.

## Without a proxy's health check

If the balancer in front only does passive health (marks an upstream down
after a failed request), the drain grace still helps — `Connection: close`
on every response during the grace stops keep-alive reuse — but the first
request after the listener closes is refused, and whether that is a 502 or a
retry is the balancer's setting. Enable an active check on `/_usai/ready` at
an interval the grace covers; it is the cheapest part of the whole setup.
