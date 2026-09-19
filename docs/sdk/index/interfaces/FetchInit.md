[@sakaladev/usai](../../README.md) / [index](../README.md) / FetchInit

# Interface: FetchInit

Options for [HttpClientHandle.fetch](HttpClientHandle.md#fetch).

## Properties

| Property | Type | Description |
| ------ | ------ | ------ |
| <a id="method"></a> `method?` | `string` | Default `GET`. |
| <a id="headers"></a> `headers?` | `Record`\<`string`, `string`\> | - |
| <a id="body"></a> `body?` | `string` | A string body, or a JSON value (serialized, `content-type: application/json`). |
| <a id="json"></a> `json?` | `unknown` | - |
| <a id="timeoutms"></a> `timeoutMs?` | `number` | Lower than the resource's timeout only. |
