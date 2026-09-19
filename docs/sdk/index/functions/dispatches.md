[@sakaladev/usai](../../README.md) / [index](../README.md) / dispatches

# Function: dispatches()

```ts
function dispatches(from: Workload, ...to: Workload[]): Workload;
```

Record that `from` invokes or dispatches the tasks `to`, so that `usai
graph`, `inspect` and the reference page show the edge (the runtime
refuses a dispatch to a task that does not exist either way). Returns
`from`, so it wraps a declaration in place.

## Parameters

| Parameter | Type |
| ------ | ------ |
| `from` | [`Workload`](../interfaces/Workload.md) |
| ...`to` | [`Workload`](../interfaces/Workload.md)[] |

## Returns

[`Workload`](../interfaces/Workload.md)
