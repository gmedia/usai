[@sakaladev/usai](../../README.md) / [index](../README.md) / publishes

# Function: publishes()

```ts
function publishes(from: Workload, ...topics: (string | Workload)[]): Workload;
```

Record that `from` publishes to queue topics (names, or the consuming
`queue.consume` workloads) with [QueueHandle.publish](../interfaces/QueueHandle.md#publish), for `usai
graph` and the reference page (the consumer's page lists its
publishers). Annotation only; the publish itself is `ctx.queue.publish`.
Returns `from`, so it wraps a declaration in place.

## Parameters

| Parameter | Type |
| ------ | ------ |
| `from` | [`Workload`](../interfaces/Workload.md) |
| ...`topics` | (`string` \| [`Workload`](../interfaces/Workload.md))[] |

## Returns

[`Workload`](../interfaces/Workload.md)
