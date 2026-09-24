[@sakaladev/usai](../../README.md) / [index](../README.md) / publishes

# Function: publishes()

```ts
function publishes<W extends Workload>(from: W, ...topics: (string | Workload)[]): W;
```

Record that `from` publishes to queue topics (names, or the consuming
`queue.consume` workloads) with [QueueHandle.publish](../interfaces/QueueHandle.md#publish), for `usai
graph` and the reference page (the consumer's page lists its
publishers). Annotation only; the publish itself is `ctx.queue.publish`.
Returns `from` with its own type, so it wraps a declaration in place
without erasing what a task's handler returns.

## Type Parameters

| Type Parameter |
| ------ |
| `W` *extends* [`Workload`](../interfaces/Workload.md) |

## Parameters

| Parameter | Type |
| ------ | ------ |
| `from` | `W` |
| ...`topics` | (`string` \| [`Workload`](../interfaces/Workload.md))[] |

## Returns

`W`
