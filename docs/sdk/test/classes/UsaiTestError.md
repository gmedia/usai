[@sakaladev/usai](../../README.md) / [test](../README.md) / UsaiTestError

# Class: UsaiTestError

Thrown when the harness itself fails: the binary is missing, the
runtime did not start, a control request was refused.

## Extends

- `Error`

## Constructors

### Constructor

```ts
new UsaiTestError(message: string, outcome?: WorkOutcome<unknown>): UsaiTestError;
```

#### Parameters

| Parameter | Type |
| ------ | ------ |
| `message` | `string` |
| `outcome?` | [`WorkOutcome`](../interfaces/WorkOutcome.md)\<`unknown`\> |

#### Returns

`UsaiTestError`

#### Overrides

```ts
Error.constructor
```

## Properties

| Property | Modifier | Type |
| ------ | ------ | ------ |
| <a id="outcome"></a> `outcome` | `readonly` | [`WorkOutcome`](../interfaces/WorkOutcome.md)\<`unknown`\> \| `undefined` |
