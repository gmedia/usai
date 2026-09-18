# Queue messages failing / dead letters

A consumer that throws asks for the next attempt; the message is retried
per its `retry` declaration (attempts, fixed or exponential backoff) and
then **dead-lettered**: state `dead` in `usai_queue`, `last_error` set.

## What you see

- Log: `message failed; retrying queue=<topic> message=<id> attempt=<n>
  delay_ms=<d> error=<message>` per attempt, then `message dead-lettered
  … attempt=<last> error=…`.
- Metrics/status: `usai_queue_messages_total{state="retried"}` per attempt,
  `{state="dead"}` once; in `/_usai/status`, `revisions[].queue`.
- Database: `select * from usai_queue where state = 'dead'` has the payload
  and the error; the application's own record (the invoicing example writes
  `webhook_deliveries` per attempt) shows what each attempt saw.

## What to do

Fix the cause (the endpoint, the payload), then requeue: `update usai_queue
set state = 'ready', attempts = 0, last_error = null where id = …`. Messages
that must never be lost belong in the queue (durable); `ctx.tasks.dispatch`
is not durable (ADR-0010) and its failures are a WARN line only.
