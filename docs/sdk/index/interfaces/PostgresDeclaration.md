[@sakaladev/usai](../../README.md) / [index](../README.md) / PostgresDeclaration

# Interface: PostgresDeclaration\<Name *extends* `string` = `string`\>

A PostgreSQL pool owned by the runtime. Each operation leases one
connection; reuse follows terminal proof (contract C5).

## Extends

- [`ResourceDeclaration`](ResourceDeclaration.md)\<`Name`, [`PostgresHandle`](PostgresHandle.md)\>

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
| <a id="kind"></a> `kind` | `readonly` | `"postgres"` | - | [`ResourceDeclaration`](ResourceDeclaration.md).[`kind`](ResourceDeclaration.md#kind) | - |
