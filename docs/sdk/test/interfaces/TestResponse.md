[@sakaladev/usai](../../README.md) / [test](../README.md) / TestResponse

# Interface: TestResponse

One HTTP response from the application under test: `body` is the
parsed JSON when the response was JSON, otherwise the text.

## Properties

| Property | Type | Description |
| ------ | ------ | ------ |
| <a id="status"></a> `status` | `number` | - |
| <a id="headers"></a> `headers` | `Record`\<`string`, `string`\> | - |
| <a id="body"></a> `body` | `unknown` | - |
| <a id="text"></a> `text` | `string` | - |
| <a id="bytes"></a> `bytes` | `Uint8Array` | The response body's exact bytes (a downloaded file, a binary raw response). |
| <a id="violations"></a> `violations` | `string`[] | Lifecycle violations the request's world committed (`detached_work`, …), from the `x-usai-lifecycle` header the harness's runtime exposes. A request that followed the rules has `[]`; assert on it. |
