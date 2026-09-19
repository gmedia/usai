[@sakaladev/usai](../../README.md) / [index](../README.md) / ServiceOptions

# Interface: ServiceOptions

Options for [service](../functions/service.md).

## Properties

| Property | Type | Description |
| ------ | ------ | ------ |
| <a id="resources"></a> `resources?` | [`ResourceDeclaration`](ResourceDeclaration.md)[] | Resources the service leases (per operation, like every world). |
| <a id="restart"></a> `restart?` | \{ `mode`: `"never"` \| `"on-failure"` \| `"always"`; `backoffMs?`: `number`; `maxRestarts?`: `number`; \} | What happens when the service ends. Default `never`: it stays ended until the next revision. `on-failure` restarts after a throw; `always` restarts whenever it ends. Backoff doubles per restart. |
| `restart.mode` | `"never"` \| `"on-failure"` \| `"always"` | - |
| `restart.backoffMs?` | `number` | - |
| `restart.maxRestarts?` | `number` | - |
