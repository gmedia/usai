[@sakaladev/usai](../../README.md) / [index](../README.md) / publishes

# Function: publishes()

```ts
function publishes(from: Workload, ...topics: (string | Workload)[]): Workload;
```

Record that `from` publishes to queue topics (names, or the consuming
`queue.consume` workloads), for `usai graph` and the reference page.
Returns `from`, so it wraps a declaration in place.

## Parameters

| Parameter | Type |
| ------ | ------ |
| `from` | [`Workload`](../interfaces/Workload.md) |
| ...`topics` | (`string` \| [`Workload`](../interfaces/Workload.md))[] |

## Returns

[`Workload`](../interfaces/Workload.md)
