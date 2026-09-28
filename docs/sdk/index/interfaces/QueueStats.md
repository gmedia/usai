[@sakaladev/usai](../../README.md) / [index](../README.md) / QueueStats

# Interface: QueueStats

One topic's queue, as [QueueHandle.stats](QueueHandle.md#stats) reports it.

## Properties

| Property | Modifier | Type | Description |
| ------ | ------ | ------ | ------ |
| <a id="topic"></a> `topic` | `readonly` | `string` | - |
| <a id="ready"></a> `ready` | `readonly` | `number` | Waiting to be claimed — the backlog. |
| <a id="processing"></a> `processing` | `readonly` | `number` | Claimed by a consumer and running now. |
| <a id="done"></a> `done` | `readonly` | `number` | Finished, and not pruned yet (`usai queue prune`). |
| <a id="dead"></a> `dead` | `readonly` | `number` | Out of attempts: a message an application failed to process. The runtime never deletes these on its own. |
| <a id="oldestreadyseconds"></a> `oldestReadySeconds` | `readonly` | `number` \| `null` | How long the oldest waiting message has waited, in seconds; `null` when nothing is waiting. The number an alert on "keeping up?" watches. |
| <a id="oldestclaimseconds"></a> `oldestClaimSeconds` | `readonly` | `number` \| `null` | How long the oldest claimed message has been claimed, in seconds. A number that keeps growing is a consumer that died mid-message. |
| <a id="lastdeaderror"></a> `lastDeadError` | `readonly` | `string` \| `null` | The error of the most recent dead message, or `null`. |
