[@sakaladev/usai](../../../../README.md) / [index](../../../README.md) / [StandardSchemaV1](../README.md) / Props

# Interface: Props\<Input = `unknown`, Output = `Input`\>

## Type Parameters

| Type Parameter | Default type |
| ------ | ------ |
| `Input` | `unknown` |
| `Output` | `Input` |

## Properties

| Property | Modifier | Type | Description |
| ------ | ------ | ------ | ------ |
| <a id="version"></a> `version` | `readonly` | `1` | - |
| <a id="vendor"></a> `vendor` | `readonly` | `string` | - |
| <a id="validate"></a> `validate` | `readonly` | (`value`: `unknown`) => \| [`Result`](../type-aliases/Result.md)\<`Output`\> \| `Promise`\<[`Result`](../type-aliases/Result.md)\<`Output`\>\> | - |
| <a id="types"></a> `types?` | `readonly` | [`Types`](Types.md)\<`Input`, `Output`\> | - |
| <a id="jsonschema"></a> `jsonSchema?` | `readonly` | [`JsonSchemaProps`](JsonSchemaProps.md) | Standard JSON Schema v1: present when the vendor can describe itself. |
