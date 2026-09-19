[@sakaladev/usai](../../README.md) / [index](../README.md) / dispatches

# Function: dispatches()

```ts
function dispatches(from: Workload, ...to: Workload[]): Workload;
```

Record that `from` invokes or dispatches the tasks `to`, so that `usai
graph`, `inspect` and the reference page show the edge. The annotation
is for the graph only: a dispatch to a task that is not listed here
still runs, and a dispatch to a task that does not exist is refused at
runtime (`unknown_task`) whether or not it is listed. Returns `from`, so
it wraps a declaration in place. See [QueueHandle.publish](../interfaces/QueueHandle.md#publish) and
[publishes](publishes.md) for the durable, cross-process equivalent.

## Parameters

| Parameter | Type |
| ------ | ------ |
| `from` | [`Workload`](../interfaces/Workload.md) |
| ...`to` | [`Workload`](../interfaces/Workload.md)[] |

## Returns

[`Workload`](../interfaces/Workload.md)
