[@sakaladev/usai](../../README.md) / [index](../README.md) / dispatches

# Function: dispatches()

```ts
function dispatches<W extends Workload>(from: W, ...to: Workload[]): W;
```

Record that `from` invokes or dispatches the tasks `to`, so that `usai
graph`, `inspect` and the reference page show the edge. The annotation
is for the graph only: a dispatch to a task that is not listed here
still runs, and a dispatch to a task that does not exist is refused at
runtime (`unknown_task`) whether or not it is listed. Returns `from`
with its own type — a task wrapped here is still a [TypedWorkload](../interfaces/TypedWorkload.md),
so `ctx.tasks.invoke` still resolves its handler's output — so it wraps a
declaration in place. See [QueueHandle.publish](../interfaces/QueueHandle.md#publish) and
[publishes](publishes.md) for the durable, cross-process equivalent.

## Type Parameters

| Type Parameter |
| ------ |
| `W` *extends* [`Workload`](../interfaces/Workload.md) |

## Parameters

| Parameter | Type |
| ------ | ------ |
| `from` | `W` |
| ...`to` | [`Workload`](../interfaces/Workload.md)[] |

## Returns

`W`
