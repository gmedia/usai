[@sakaladev/usai](../../README.md) / [index](../README.md) / ResourceDeclaration

# Interface: ResourceDeclaration\<Name *extends* `string` = `string`, Handle = `unknown`\>

What `postgres(...)`, `cache.local(...)` and `httpClient(...)` return.
A plain object: the build reads it into the manifest, the runtime owns
the resource it names, and a workload lists it under `resources`.

## Extended by

- [`CacheLocalDeclaration`](CacheLocalDeclaration.md)
- [`PostgresDeclaration`](PostgresDeclaration.md)
- [`HttpClientDeclaration`](HttpClientDeclaration.md)

## Type Parameters

| Type Parameter | Default type |
| ------ | ------ |
| `Name` *extends* `string` | `string` |
| `Handle` | `unknown` |

## Properties

| Property | Modifier | Type | Description |
| ------ | ------ | ------ | ------ |
| <a id="name-1"></a> `name` | `readonly` | `Name` | - |
| <a id="kind"></a> `kind` | `readonly` | `string` | - |
| <a id="config"></a> `config` | `readonly` | `Record`\<`string`, `unknown`\> | Normalized, secret-free configuration. |
| <a id="env"></a> `env` | `readonly` | readonly `string`[] | Env variable names that participate in the resource identity. |
| <a id="methods"></a> `methods` | `readonly` | readonly `string`[] | Methods the in-world proxy exposes. |
