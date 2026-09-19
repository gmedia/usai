[@sakaladev/usai](../../README.md) / [test](../README.md) / TestResponse

# Interface: TestResponse

One HTTP response from the application under test: `body` is the
parsed JSON when the response was JSON, otherwise the text.

## Properties

| Property | Type |
| ------ | ------ |
| <a id="status"></a> `status` | `number` |
| <a id="headers"></a> `headers` | `Record`\<`string`, `string`\> |
| <a id="body"></a> `body` | `unknown` |
| <a id="text"></a> `text` | `string` |
