[@sakaladev/usai](../../README.md) / [index](../README.md) / HttpClientDeclaration

# Interface: HttpClientDeclaration\<Name *extends* `string` = `string`\>

Outbound HTTP, declared: the runtime owns the client (pool, TLS roots,
timeouts), every request is an operation owned by the world, and the
destination is visible in `usai graph` and the API docs. There is no
global `fetch` inside a world.

## Extends

- [`ResourceDeclaration`](ResourceDeclaration.md)\<`Name`, [`HttpClientHandle`](HttpClientHandle.md)\>

## Type Parameters

| Type Parameter | Default type |
| ------ | ------ |
| `Name` *extends* `string` | `string` |

## Properties

| Property | Modifier | Type | Description | Overrides | Inherited from |
| ------ | ------ | ------ | ------ | ------ | ------ |
| <a id="name-1"></a> `name` | `readonly` | `Name` | - | - | [`ResourceDeclaration`](ResourceDeclaration.md).[`name`](ResourceDeclaration.md#name-1) |
| <a id="config"></a> `config` | `readonly` | `Record`\<`string`, `unknown`\> | Normalized, secret-free configuration. | - | [`ResourceDeclaration`](ResourceDeclaration.md).[`config`](ResourceDeclaration.md#config) |
| <a id="env"></a> `env` | `readonly` | readonly `string`[] | Env variable names that participate in the resource identity. | - | [`ResourceDeclaration`](ResourceDeclaration.md).[`env`](ResourceDeclaration.md#env) |
| <a id="methods"></a> `methods` | `readonly` | readonly `string`[] | Methods the in-world proxy exposes. | - | [`ResourceDeclaration`](ResourceDeclaration.md).[`methods`](ResourceDeclaration.md#methods) |
| <a id="kind"></a> `kind` | `readonly` | `"http.client"` | - | [`ResourceDeclaration`](ResourceDeclaration.md).[`kind`](ResourceDeclaration.md#kind) | - |
