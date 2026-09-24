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
| <a id="timeout"></a> `timeout?` | `string` \| `number` | How long this service may run before the world is cancelled (`"30m"`, `"4s"`, or milliseconds). **Undeclared there is none**, which is what a service usually wants — it runs until the revision retires. Declare one for a loop that should not outlive a bound (a nightly importer written as a service, say); with `restart: { mode: "always" }` it becomes a supervised cycle rather than a runaway. |
