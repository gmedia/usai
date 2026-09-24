[@sakaladev/usai](../../README.md) / [test](../README.md) / WorldLogLine

# Interface: WorldLogLine

One line a world wrote, as the outcome carries it.

## Properties

| Property | Type | Description |
| ------ | ------ | ------ |
| <a id="level"></a> `level` | `string` | - |
| <a id="message"></a> `message` | `string` | - |
| <a id="fields"></a> `fields` | `Record`\<`string`, `unknown`\> \| `null` | The trailing object of `ctx.log.info("paid", { id })`, parsed. |
| <a id="workload"></a> `workload` | `string` | - |
| <a id="world"></a> `world` | `string` | - |
| <a id="requestid"></a> `requestId` | `string` \| `null` | The request the world ran under, when it had one. |
