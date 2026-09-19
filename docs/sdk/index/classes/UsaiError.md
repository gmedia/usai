[@sakaladev/usai](../../README.md) / [index](../README.md) / UsaiError

# Class: UsaiError

An error with a `code` and an HTTP `status`. Thrown from a handler it
becomes the response `{ "error": { code, message, details? } }` with
that status; from a task or queue message it is the outcome's error.
Any other thrown value is a 500 `internal` with a sanitized message.

The runtime's own codes, which a client can meet on any operation:
`validation_failed` (400; `details.slot`, `details.issues[]` with `path`
as a JSON pointer and `message`), `unauthorized` (401),
`route_not_found` (404), `capacity_exhausted` (503), `unavailable`
(503, a dependency), `deadline_exceeded` (504), `internal` (500),
`detached_work` (500 only when an *operation* the handler started was
still in flight when it returned — its side effect is unknown; a live
timer or promise is cancelled and the returned response is committed,
with the violation logged and, under `--diagnostics`, in the
`x-usai-lifecycle` header), and inside a world `resource_not_declared`,
`unknown_task`, `cancelled` (499).

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
