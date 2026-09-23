# Queue messages failing / dead letters

A consumer that throws asks for the next attempt; the message is retried
per its `retry` declaration (attempts, fixed or exponential backoff) and
then **dead-lettered**: state `dead` in `usai_queue`, `last_error` set.

## What you see

- Log: `message failed; retrying queue=<topic> id=<id> attempt=<n>
  delay_ms=<d> error=<message>` per attempt, then `message dead-lettered
  … attempt=<last> error=…`.
- Metrics/status: `usai_queue_messages_total{state="retried"}` per attempt,
  `{state="dead"}` once; in `/_usai/status`, `revisions[].queue`.
- Database: `select * from usai_queue where state = 'dead'` has the payload
  and the error; the application's own record (the invoicing example writes
  `webhook_deliveries` per attempt) shows what each attempt saw.

**A dead letter whose `last_error` reads `consumer lost: claimed by … at …,
never completed`** is a different story: the message was claimed and its
consumer never recorded an outcome. Two causes, and the row tells them
apart from the application's own records:

- the consumer really died (a crash, an OOM kill, a `kill -9`, a lost host)
  — the work did not finish, and redelivery is what you want;
- the consumer finished but **could not write the outcome** (the pool was
  saturated, the database blipped). The runtime retries that write once and
  then logs `could not record the message's outcome; the row stays
  'processing' …`; grep for that line at the message's `locked_at`. If it is
  there, the handler already ran — requeue only if the handler is not
  idempotent to a second run, and note that with `retry.maxAttempts` at its
  default of 1 there is no second attempt, which is why the row went
  straight to `dead`.

## What to do

First look at what is actually there:

```bash
usai --root /srv/app queue status                 # per topic and state, with ages
usai --root /srv/app queue status --json | jq     # for a script or an alert
```

`ready` is depth. A `processing` row older than the consumer's deadline is a
lost consumer — the sweeper reclaims it; you do not have to. `dead` is what
this page is about.

Fix the cause (the endpoint, the payload), then requeue. There is no verb for
this on purpose: requeueing is a decision about *which* failures were
transient, and the runtime will not guess.

```sql
-- one message, after you have read its last_error
update usai_queue set state = 'ready', attempts = 0, last_error = null where id = 42;
-- a batch that failed for one cause you have fixed
update usai_queue set state = 'ready', attempts = 0, last_error = null
 where topic = 'invoice.issued' and state = 'dead' and last_error like '%502%';
```

Then keep the table from growing forever — the runtime never prunes it:

```bash
usai --root /srv/app queue prune --state done --older-than 30d --yes
usai --root /srv/app queue prune --state dead --older-than 90d --dry-run
```

`--dry-run` counts and deletes nothing. The age is the message's
`created_at` — the one column that does not move; `available_at` is pushed
forward by every retry. Keep `dead` longer than `done`: it is the evidence
for the next incident.

Messages that must never be lost belong in the queue (durable);
`ctx.tasks.dispatch` is not durable (ADR-0010) and its failures are a WARN
line only.
