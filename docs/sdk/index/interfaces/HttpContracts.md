[@sakaladev/usai](../../README.md) / [index](../README.md) / HttpContracts

# Interface: HttpContracts

The schema slots of an HTTP endpoint. Any Standard Schema
(`zod`, `valibot`, `arktype`, …) works; slots whose schema can describe
itself as JSON Schema are validated before a world exists, the others
inside it.

## Extended by

- [`HttpOptions`](HttpOptions.md)

## Properties

| Property | Type |
| ------ | ------ |
| <a id="params"></a> `params?` | [`AnySchema`](../type-aliases/AnySchema.md) |
| <a id="query"></a> `query?` | [`AnySchema`](../type-aliases/AnySchema.md) |
| <a id="headers"></a> `headers?` | [`AnySchema`](../type-aliases/AnySchema.md) |
| <a id="body"></a> `body?` | [`AnySchema`](../type-aliases/AnySchema.md) |
| <a id="response"></a> `response?` | \| [`AnySchema`](../type-aliases/AnySchema.md) \| `Record`\<`number`, [`AnySchema`](../type-aliases/AnySchema.md)\> |
