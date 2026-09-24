# Online schema change: rename a column with no downtime

The change this page walks through is the one everybody has to do eventually:
`invoices.due_date` (a `date`) becomes `invoices.due_at` (a `timestamptz`),
on a table with tens of millions of rows, behind a load balancer with **two
replicas and one PostgreSQL**, with no failed requests.

It is written because a database-owning engineer worked it out against this
runtime and found that the documents said nothing about it (round 24,
`docs/STATUS.md`) — the procedure is standard expand/contract, but *which
parts of it this runtime makes safe, and which it leaves to you*, was not
written down anywhere.

Read `deploy-and-rollback.md` first if you have not; this page assumes the
two-axis rollback model (artifact and binary) it describes.

## What the runtime gives you, and what it does not

**Each migration file is its own transaction.** The runtime runs
`BEGIN; SELECT pg_advisory_xact_lock(…); INSERT INTO usai_migrations …;
<your SQL>; COMMIT;`. Three consequences, and they decide whether your
change is safe:

- **An `ALTER TABLE` and a backfill in the same file hold `ACCESS
  EXCLUSIVE` for the backfill's whole duration.** That is the classic
  outage. Keep them in separate files — the per-file boundary is what saves
  you, and only if you use it.
- **A file that takes minutes blocks writers to that table for minutes**,
  while HTTP reads keep answering 200 and `/_usai/ready` stays green. The
  balancer will keep feeding writes into the block. Do not put a backfill in
  a migration at all; see *Release 3*.
- **The ledger row is inserted before your SQL runs**, so
  `usai_migrations.applied_at` is the transaction's *start*, not its end.
  When you correlate a migration against an incident, add the duration.

**Several `usai db migrate` at once are safe.** The advisory lock plus the
ledger's primary key make each file apply exactly once, so an initContainer
on every replica needs no ordering machinery (`../deploy/k8s/README.md`).
The losers block for the whole run, though: with a twenty-minute migration,
every other replica's initContainer waits twenty minutes. Run long
migrations as a separate job, not as an initContainer.

**A migration can opt out of its transaction.** `CREATE INDEX
CONCURRENTLY`, `ALTER TYPE … ADD VALUE`, `VACUUM` and `REINDEX
CONCURRENTLY` cannot run inside one. Put `-- usai: no-transaction` on a line
of its own and the file runs outside the transaction:

```sql
-- usai: no-transaction
create index concurrently if not exists invoices_due_at_idx on invoices (due_at);
```

The ledger row cannot then be written atomically with the work, so a process
that dies between the two runs the file again — **the SQL must be
idempotent**. `IF NOT EXISTS` is how. And note PostgreSQL's own rule: a
concurrent index build that fails leaves an *invalid* index, which you must
`DROP INDEX` by hand before the retry.

**Nothing in the runtime knows your schema.** `usai inspect`, `usai graph`
and activation never look at the database, so an artifact whose SQL
references a column that no longer exists is discovered at request time, as
a `500`, with readiness still green. **Readiness is not a canary**: alert on
`usai_http_responses_total{class="5xx"}`.

## The procedure

### Release 1 — expand (schema only, seconds)

```sql
-- migrations/010_add_due_at.sql
alter table invoices add column due_at timestamptz;
```

One file, nothing else in it. Deploy order: `usai db migrate`, then roll the
replicas. The old artifact ignores the new column.

### Release 2 — write both, read leniently

The application writes `due_date` *and* `due_at` in the same
`sql.transaction`, and reads
`coalesce(due_at, due_date::timestamp at time zone 'UTC')`.

Two things that bite:

- **Declare the new field nullable in the response contract for the whole
  expand window.** A route that declares `due: z.string()` and meets a row
  the backfill has not reached answers `500 response_contract_violation`,
  naming the field. That is the contract working — it does not know about
  your migration window, so widen it deliberately and narrow it again in
  release 4.
- **Never write `due_date::timestamptz`.** That cast uses the session's
  `TimeZone`, and pooled connections are reset with `RESET ALL`, so it is
  the *server's* zone — midnight local, not midnight UTC. There is no
  supported way to pin a session zone (`SET LOCAL` inside a transaction is
  the only one). Write the zone into the cast:
  `due_date::timestamp at time zone 'UTC'`.

### Release 2b — the index

```sql
-- migrations/011_due_at_index.sql
-- usai: no-transaction
create index concurrently if not exists invoices_due_at_idx on invoices (due_at);
```

### Release 3 — the backfill

A `command()` with a **declared** timeout, one committed statement per
batch. Not a migration, and not a single `UPDATE`:

```ts
export const backfill = command("backfill", { timeout: "4h", resources: [db] }, async (ctx) => {
  const batch = Number(ctx.args[0] ?? "1000");
  let total = 0;
  for (;;) {
    const n = await ctx.resources.main.execute(
      `update invoices set due_at = due_date::timestamp at time zone 'UTC'
         where id in (select id from invoices where due_at is null order by id limit $1)`,
      [batch],
    );
    total += n;
    if (n === 0 || ctx.signal.aborted) break;
  }
  return { total, done: !ctx.signal.aborted };
});
```

- **`timeout:` is not optional.** The default is 30 s, and past it the
  command stops with what it has committed committed. `usai inspect` prints
  every workload's effective deadline and where it came from.
- **`ctx.signal.aborted` between batches is what makes it deploy-safe.** A
  drain asks the world to stop; the loop returns, the drain finishes, and
  the return value says how far it got. Write progress to a table as well if
  the job is long — the runtime has no job-shaped progress surface, and a
  cancelled invoke returns no value.
- **Run it where the source is.** `usai app` needs a project; a production
  image has only `.usai/build`. Either run it from a checkout / the `-dev`
  image against the production `DATABASE_URL`, or invoke it on a replica
  through the control surface (`POST /invoke`, `../CONTROL-API.md`).
- It is idempotent by construction (`where due_at is null`), so re-running
  after an interruption is the recovery.

### Release 4 — read the new column only

Read `due_at`, make the contract non-nullable again, stop writing
`due_date`.

### Release 5 — contract, days later

```sql
-- migrations/020_drop_due_date.sql
alter table invoices drop column due_date;
```

**Before you run it, check that no artifact you could roll back to still
reads `due_date`.** With the old artifact serving and the column gone, every
request to its routes is a `500` (`sql_42703` in the log) and readiness
stays green.

## The queue, across all of this

If a message carries the field, **extend the consumer's schema a release
before the publisher changes**. A rolling deploy gives you a window where
replica A publishes the new shape and replica B's consumer rejects it, and a
message that fails its contract is **dead-lettered, not retried** — no world
runs for it, so `retry` never applies. Measured in that window: of twenty
messages, thirteen were permanently dead-lettered by the old replica and
seven consumed by the new one.

So: add `dueAt` **alongside** `dueDate` in the message, ship the consumer
first, and remove the old field a release later. Recovery from the dead
letters is hand-written SQL (`queue-dead-letter.md`).

## Checks worth having in the deploy

```bash
usai db status --check --artifact /app/.usai/build   # non-zero if anything is pending
usai db status --artifact /app/.usai/build           # non-zero if a file was edited after it was applied
```

Alert on `usai_http_responses_total{class="5xx"}` and on
`increase(usai_queue_messages_total{state=~"dead|invalid"}[15m]) > 0` for the
window (`metrics.md`). Readiness tells you the instance is up; it does not
tell you the schema and the artifact agree.
