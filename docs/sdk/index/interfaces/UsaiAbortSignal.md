[@sakaladev/usai](../../README.md) / [index](../README.md) / UsaiAbortSignal

# Interface: UsaiAbortSignal

`ctx.signal`: aborts when this world is cancelled — the client went
away, the deadline passed, the revision drained, or the owner of an
`invoke` was cancelled. Pending host operations reject with
`cancelled` at the same moment; the signal is for the handler's own
loops and cleanup.

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
