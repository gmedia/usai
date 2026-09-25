[@sakaladev/usai](../../README.md) / [index](../README.md) / ServiceOptions

# Interface: ServiceOptions\<R *extends* [`ResourceDeclaration`](ResourceDeclaration.md)[] = [`ResourceDeclaration`](ResourceDeclaration.md)[]\>

Options for [service](../functions/service.md).

## Type Parameters

| Type Parameter | Default type |
| ------ | ------ |
| `R` *extends* [`ResourceDeclaration`](ResourceDeclaration.md)[] | [`ResourceDeclaration`](ResourceDeclaration.md)[] |

## Properties

| Property | Type | Description |
| ------ | ------ | ------ |
| <a id="description"></a> `description?` | `string` | A paragraph for the reference. |
| <a id="resources"></a> `resources?` | `R` | Resources the service leases (per operation, like every world); `ctx.resources` is typed from this list. |
| <a id="restart"></a> `restart?` | \{ `mode`: `"never"` \| `"on-failure"` \| `"always"`; `backoffMs?`: `number`; `maxRestarts?`: `number`; \} | What happens when the service ends. Default `never`: it stays ended until the next revision. `on-failure` restarts after a throw; `always` restarts whenever it ends. Backoff doubles per restart. |
| `restart.mode` | `"never"` \| `"on-failure"` \| `"always"` | - |
| `restart.backoffMs?` | `number` | - |
| `restart.maxRestarts?` | `number` | - |
| <a id="exclusive"></a> `exclusive?` | `boolean` | Run **exactly one** of these across every replica. Each instance claims a lease in `usai_service_leases`; only the holder starts the world, and the others report `waiting` — healthy, ready to take over, neither stopped nor failed. The holder renews every third of `leaseMs`, so a replica that dies is replaced within about that long; one that drains gives the lease up and the next starts at once. This is what a reconciler wants — a loop comparing desired and actual state should have one writer, while two replicas exist for availability. A cron schedule's `exclusive: true` is the same idea for a tick. Losing the lease **stops** the world rather than cancelling it, so the handler sees `ctx.signal` and can finish what it holds. |
| <a id="database"></a> `database?` | [`PostgresDeclaration`](PostgresDeclaration.md)\<`string`\> | Where the lease lives; the application's first `postgres` by default. Only read when `exclusive` is set. |
| <a id="leasems"></a> `leaseMs?` | `number` | How long a claim is good for, and so roughly the takeover time after a replica dies without draining. Default 30 000 ms, minimum 3 000. |
| <a id="timeout"></a> `timeout?` | `string` \| `number` | How long this service may run before the world is cancelled (`"30m"`, `"4s"`, or milliseconds). **Undeclared there is none**, which is what a service usually wants — it runs until the revision retires. Declare one for a loop that should not outlive a bound (a nightly importer written as a service, say); with `restart: { mode: "always" }` it becomes a supervised cycle rather than a runaway. |
