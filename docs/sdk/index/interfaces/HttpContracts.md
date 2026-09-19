[@sakaladev/usai](../../README.md) / [index](../README.md) / HttpContracts

# Interface: HttpContracts

The schema slots of an HTTP endpoint. Any Standard Schema
(`zod`, `valibot`, `arktype`, …) works; slots whose schema can describe
itself as JSON Schema are validated before a world exists, the others
inside it.

## Extended by

- [`HttpOptions`](HttpOptions.md)

## Properties

| Property | Type | Description |
| ------ | ------ | ------ |
| <a id="params"></a> `params?` | [`AnySchema`](../type-aliases/AnySchema.md) | Path parameters (`/users/:id` → `{ id }`); strings before coercion. |
| <a id="query"></a> `query?` | [`AnySchema`](../type-aliases/AnySchema.md) | Query string; a repeated key arrives as an array. |
| <a id="headers"></a> `headers?` | [`AnySchema`](../type-aliases/AnySchema.md) | Request headers, lower-cased names. |
| <a id="body"></a> `body?` | [`AnySchema`](../type-aliases/AnySchema.md) | JSON request body. |
| <a id="response"></a> `response?` | \| [`AnySchema`](../type-aliases/AnySchema.md) \| `Record`\<`number`, [`AnySchema`](../type-aliases/AnySchema.md)\> | One schema (status 200) or a map of status to schema. A plain return value is encoded with the lowest declared 2xx status and checked against its schema (`response_contract_violation`, 500, otherwise); `http.response(status, body)` picks another declared status. |
