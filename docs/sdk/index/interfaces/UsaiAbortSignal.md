[@sakaladev/usai](../../README.md) / [index](../README.md) / UsaiAbortSignal

# Interface: UsaiAbortSignal

`ctx.signal`: aborts when this world is asked to stop or is cancelled.
A revision drain *asks* first — services, connections, dispatched tasks,
cron ticks and queue messages see `aborted` become true and a pending
`ctx.sleep` return, while operations keep running, so a loop can record
where it got to and return; what has not returned by the drain bound is
cancelled outright (no more code runs). A client that went away, a
deadline, or a cancelled `invoke` owner cancel outright too: pending host
operations reject with `cancelled` and the world ends.

## Properties

| Property | Modifier | Type | Description |
| ------ | ------ | ------ | ------ |
| <a id="aborted"></a> `aborted` | `readonly` | `boolean` | - |
| <a id="reason"></a> `reason` | `readonly` | `string` \| `undefined` | Why, once aborted (`deadline exceeded`, `cancelled by owner`, …). |

## Methods

### addEventListener()

```ts
addEventListener(type: "abort", listener: (reason: string) => void): void;
```

Runs at abort (immediately when already aborted).

#### Parameters

| Parameter | Type |
| ------ | ------ |
| `type` | `"abort"` |
| `listener` | (`reason`: `string`) => `void` |

#### Returns

`void`

***

### throwIfAborted()

```ts
throwIfAborted(): void;
```

Throws a `cancelled` (499) [UsaiError](../classes/UsaiError.md) once aborted.

#### Returns

`void`
