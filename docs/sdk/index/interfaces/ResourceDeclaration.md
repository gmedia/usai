[@sakaladev/usai](../../README.md) / [index](../README.md) / ResourceDeclaration

# Interface: ResourceDeclaration

What `postgres(...)`, `cache.local(...)` and `httpClient(...)` return.
A plain object: the build reads it into the manifest, the runtime owns
the resource it names, and a workload lists it under `resources`.

## Extended by

- [`CacheLocalDeclaration`](CacheLocalDeclaration.md)
- [`PostgresDeclaration`](PostgresDeclaration.md)
- [`HttpClientDeclaration`](HttpClientDeclaration.md)

## Properties

| Property | Modifier | Type | Description |
| ------ | ------ | ------ | ------ |
| <a id="__usai"></a> `__usai` | `readonly` | `"resource"` | - |
| <a id="name"></a> `name` | `readonly` | `string` | - |
| <a id="kind"></a> `kind` | `readonly` | `string` | - |
| <a id="config"></a> `config` | `readonly` | `Record`\<`string`, `unknown`\> | Normalized, secret-free configuration. |
| <a id="env"></a> `env` | `readonly` | readonly `string`[] | Env variable names that participate in the resource identity. |
| <a id="methods"></a> `methods` | `readonly` | readonly `string`[] | Methods the in-world proxy exposes. |
