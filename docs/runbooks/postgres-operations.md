# PostgreSQL: backup, restore, and the tables the runtime owns

Your database is your database — this page is only about what the runtime
puts in it, what a restore has to get right, and the two rotations that
otherwise happen at the worst moment. Backup strategy itself
(`pg_dump` vs physical, PITR, retention) is PostgreSQL's own documentation,
and nothing here changes it.

## What the runtime keeps in your database

Three plain tables in the resource's database, all in `public`:

| Table | Written by | On restore |
|---|---|---|
| `usai_migrations` | `usai db migrate` | **Must match the schema.** It is the ledger: a restore that has the tables but not the ledger will re-run every migration; one with the ledger but an older schema will skip migrations it never applied. |
| `usai_queue` | a publish or a consumer, created on first use | Restoring it replays work: rows in `ready` are delivered again, rows in `processing` are reclaimed by the sweeper after their deadline. Decide before the restore whether you want that. |
| `usai_cron_ticks` | the first tick of an `exclusive: true` schedule | Only the recent rows matter (they are the "who ran this minute" claims). Losing it means one duplicate tick at most. |

They are in the same dump as the rest of the schema; nothing special is
needed to back them up. **Never edit `usai_migrations` by hand** — if a
migration must be undone, write the migration that undoes it.

## Restoring

1. Restore into a database nothing is serving. The runtime opens its pool at
   activation, not at first request, so a restore under a running instance
   fights it.
2. `usai db status --artifact /app/.usai/build` — the ledger against the
   migrations the artifact carries. Everything applied and nothing pending is
   the only state to start from.
3. Decide about `usai_queue` **before** starting the instances:
   - keeping it replays the work it held (at-least-once, so a handler that is
     not idempotent will double-charge something);
   - emptying it (`truncate usai_queue`) loses that work silently.
   There is no third option the runtime can pick for you.
4. Start one instance, check `/_usai/ready` and `resources[].ready`, then let
   the balancer in.

A **point-in-time restore** is the same list, with one addition: the queue
table is a clock of its own. Restoring the database to T while the
application's downstream effects (emails sent, webhooks delivered) happened
after T means those messages run again.

## Connection limits

Each instance holds up to `pool.max` connections **per postgres resource**
(default 16; `usai inspect` prints the effective number). Size the server for
`instances × pool.max × resources`, plus what your migrations and your own
tools take. A server that runs out answers `FATAL: sorry, too many clients`,
which the runtime reports as a connection-level failure: the resource goes
unready, `/_usai/ready` fails, and the balancer takes the instance out
(`postgres-down.md`).

## Rotating the database password

The URL is read at activation, so a password change is a restart:

1. Create the new password on the server (PostgreSQL allows both for a
   moment if you add a second role, or accept the gap).
2. Update `DATABASE_URL` where the process reads it.
3. Rolling restart (`deploy-and-rollback.md`). Each instance opens its pool
   with the new value; the old connections go with the old process.

The resource's identity is the config **plus the resolved environment**, so
the log says what happened: a new `resource opened … fingerprint=…` after the
restart is the new password, and `resource reused` would mean the process
never saw the change.

## TLS: certificate and CA rotation

The runtime verifies the server certificate **always** — Mozilla's roots plus
whatever `tls.caFile` / `PGSSLROOTCERT` adds (`sslmode=require` to insist on
TLS, `prefer` is the default).

- **The server's certificate expires.** Nothing warns you: the first
  connection after expiry fails, the resource goes unready and the instance
  leaves the balancer. Watch the expiry with your certificate tooling, not
  with this runtime.
- **The CA changes** (a managed provider rotating its chain). Add the new CA
  to the bundle **before** the server starts serving the new chain — a bundle
  may hold both — then restart. The file is read at activation.
- A private CA is a file the process must be able to read: in a container it
  is a mount, and a read-only filesystem is fine.

## What this runtime does not do

No backups, no failover, no connection proxying, no read replicas. One
`postgres` resource is one connection URL; if your topology needs a pooler
(PgBouncer) or a failover endpoint, point the URL at it — the runtime's
pooling sits on top and its terminal-proof rules are unchanged, but
**transaction-level pooling breaks `transaction()`**: use session pooling or
point at the primary directly.
