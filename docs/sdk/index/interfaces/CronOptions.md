[@sakaladev/usai](../../README.md) / [index](../README.md) / CronOptions

# Interface: CronOptions\<R *extends* [`ResourceDeclaration`](ResourceDeclaration.md)[] = [`ResourceDeclaration`](ResourceDeclaration.md)[]\>

Options for [cron](../functions/cron.md).

## Extends

- `BodylessPolicies`

## Type Parameters

| Type Parameter | Default type |
| ------ | ------ |
| `R` *extends* [`ResourceDeclaration`](ResourceDeclaration.md)[] | [`ResourceDeclaration`](ResourceDeclaration.md)[] |

## Properties

| Property | Type | Description | Inherited from |
| ------ | ------ | ------ | ------ |
| <a id="timeout"></a> `timeout?` | `string` \| `number` | Per-invocation deadline (`"5s"`, `"500ms"`, or milliseconds). For **finite** work the world is cancelled when it passes: an HTTP caller gets 504, an `invoke` rejects with `deadline_exceeded`, a queue message counts as a failed attempt. Undeclared, finite work gets the runtime default of 30 s. For **connection-bound and persistent** work — a stream, a socket, a service — there is **no default**, because one that ended after 30 s would be useless. A deadline you declare is honoured, and it **stops** the world rather than cancelling it: `ctx.signal` aborts, a pending `ctx.sleep` returns, the handler can finish what it is doing, and a socket's connection is closed so its `close` handler runs. It has one second to unwind and is then cancelled, so a handler that never checks `ctx.signal` still stops near the bound you declared. For a stream the client keeps the `200` it already has and the body stops there — with no trailer and no error, so end an export with a sentinel the reader requires. | [`WorkloadPolicies`](WorkloadPolicies.md).[`timeout`](WorkloadPolicies.md#timeout) |
| <a id="concurrency"></a> `concurrency?` | `number` | How many worlds of this workload may run at once. Past the bound the next one is refused, never queued: an HTTP request gets 503 `capacity_exhausted`, `ctx.tasks.invoke`/`dispatch` reject with the same code in the caller's world, a queue consumer simply claims fewer messages (ADR-0012). | [`WorkloadPolicies`](WorkloadPolicies.md).[`concurrency`](WorkloadPolicies.md#concurrency) |
| <a id="description"></a> `description?` | `string` | A paragraph for the reference. | - |
| <a id="schedule"></a> `schedule` | `string` | Cron expression, UTC: five fields (`minute hour day-of-month month day-of-week`) or six with leading seconds. Validated at install. | - |
| <a id="overlap"></a> `overlap?` | `"allow"` \| `"skip"` | What to do when a tick is due while the previous one still runs. `skip` (default) drops the tick; `allow` starts another world. | - |
| <a id="exclusive"></a> `exclusive?` | \| `boolean` \| \{ `database`: [`PostgresDeclaration`](PostgresDeclaration.md); \} | Exactly one instance runs each tick, however many replicas schedule: every scheduler claims the tick in PostgreSQL (`usai_cron_ticks`, one row per schedule and scheduled time) and only the claimant runs the handler. `true` claims through the application's first `postgres` resource; `{ database }` names one. Without it every instance that schedules runs every tick (`SUPPORTED.md`). | - |
| <a id="resources"></a> `resources?` | `R` | Resources the tick leases; `ctx.resources` is typed from this list. | - |
