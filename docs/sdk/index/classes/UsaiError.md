[@sakaladev/usai](../../README.md) / [index](../README.md) / UsaiError

# Class: UsaiError

An error with a `code` and an HTTP `status`. Thrown from a handler it
becomes the response `{ "error": { code, message, details? } }` with
that status; from a task or queue message it is the outcome's error.
Any other thrown value is a 500 `internal` with a sanitized message.

## Extends

- `Error`

## Constructors

### Constructor

```ts
new UsaiError(
   code: string, 
   status: number, 
   message: string, 
   details?: unknown
): UsaiError;
```

#### Parameters

| Parameter | Type |
| ------ | ------ |
| `code` | `string` |
| `status` | `number` |
| `message` | `string` |
| `details?` | `unknown` |

#### Returns

`UsaiError`

#### Overrides

```ts
Error.constructor
```

## Properties

| Property | Modifier | Type |
| ------ | ------ | ------ |
| <a id="usai"></a> `usai` | `readonly` | [`UsaiErrorShape`](../interfaces/UsaiErrorShape.md) |
