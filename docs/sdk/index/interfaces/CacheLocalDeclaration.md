[@sakaladev/usai](../../README.md) / [index](../README.md) / CacheLocalDeclaration

# Interface: CacheLocalDeclaration

Shared across worlds, local to the runtime, not durable, may disappear on
restart.

## Extends

- [`ResourceDeclaration`](ResourceDeclaration.md)

## Properties

| Property | Modifier | Type | Description | Overrides | Inherited from |
| ------ | ------ | ------ | ------ | ------ | ------ |
| <a id="__usai"></a> `__usai` | `readonly` | `"resource"` | - | - | [`ResourceDeclaration`](ResourceDeclaration.md).[`__usai`](ResourceDeclaration.md#__usai) |
| <a id="name"></a> `name` | `readonly` | `string` | - | - | [`ResourceDeclaration`](ResourceDeclaration.md).[`name`](ResourceDeclaration.md#name) |
| <a id="config"></a> `config` | `readonly` | `Record`\<`string`, `unknown`\> | Normalized, secret-free configuration. | - | [`ResourceDeclaration`](ResourceDeclaration.md).[`config`](ResourceDeclaration.md#config) |
| <a id="env"></a> `env` | `readonly` | readonly `string`[] | Env variable names that participate in the resource identity. | - | [`ResourceDeclaration`](ResourceDeclaration.md).[`env`](ResourceDeclaration.md#env) |
| <a id="methods"></a> `methods` | `readonly` | readonly `string`[] | Methods the in-world proxy exposes. | - | [`ResourceDeclaration`](ResourceDeclaration.md).[`methods`](ResourceDeclaration.md#methods) |
| <a id="kind"></a> `kind` | `readonly` | `"cache.local"` | - | [`ResourceDeclaration`](ResourceDeclaration.md).[`kind`](ResourceDeclaration.md#kind) | - |
