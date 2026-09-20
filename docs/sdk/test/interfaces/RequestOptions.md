[@sakaladev/usai](../../README.md) / [test](../README.md) / RequestOptions

# Interface: RequestOptions

Options for `app.http.*`.

## Properties

| Property | Type | Description |
| ------ | ------ | ------ |
| <a id="body"></a> `body?` | `unknown` | A JSON value (serialized, `content-type: application/json`), a raw string, or bytes (`Uint8Array`, sent as-is — set `content-type` in `headers`, `application/octet-stream` otherwise: a file upload to an `http.raw` route). |
| <a id="headers"></a> `headers?` | `Record`\<`string`, `string`\> | - |
| <a id="query"></a> `query?` | `Record`\<`string`, `string` \| `number` \| `boolean`\> | - |
