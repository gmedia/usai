[@sakaladev/usai](../../README.md) / [test](../README.md) / TestSocketRefused

# Class: TestSocketRefused

Thrown by `TestApp.socket` when the server answered the upgrade with an
HTTP status instead of `101` (a refused credential is a `401`).

## Extends

- `Error`

## Constructors

### Constructor

```ts
new TestSocketRefused(status: number, body: string): TestSocketRefused;
```

#### Parameters

| Parameter | Type |
| ------ | ------ |
| `status` | `number` |
| `body` | `string` |

#### Returns

`TestSocketRefused`

#### Overrides

```ts
Error.constructor
```

## Properties

| Property | Modifier | Type |
| ------ | ------ | ------ |
| <a id="status"></a> `status` | `readonly` | `number` |
| <a id="body"></a> `body` | `readonly` | `string` |
