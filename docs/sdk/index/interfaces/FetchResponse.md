[@sakaladev/usai](../../README.md) / [index](../README.md) / FetchResponse

# Interface: FetchResponse

A completed response: the body has already arrived, so the accessors
are synchronous.

## Properties

| Property | Modifier | Type | Description |
| ------ | ------ | ------ | ------ |
| <a id="status"></a> `status` | `readonly` | `number` | - |
| <a id="ok"></a> `ok` | `readonly` | `boolean` | `status` is 2xx. |
| <a id="headers"></a> `headers` | `readonly` | `Record`\<`string`, `string`\> | Lower-cased header names. |

## Methods

### text()

```ts
text(): string;
```

#### Returns

`string`

***

### json()

```ts
json<T = unknown>(): T;
```

`JSON.parse` of the body.

#### Type Parameters

| Type Parameter | Default type |
| ------ | ------ |
| `T` | `unknown` |

#### Returns

`T`

***

### bytes()

```ts
bytes(): Uint8Array;
```

Raw bytes (text bodies are UTF-8 encoded).

#### Returns

`Uint8Array`
