[@sakaladev/usai](../../README.md) / [index](../README.md) / HttpResponse

# Interface: HttpResponse\<T = `unknown`\>

Explicit response: status, headers, and a body the runtime encodes.

## Type Parameters

| Type Parameter | Default type |
| ------ | ------ |
| `T` | `unknown` |

## Properties

| Property | Modifier | Type |
| ------ | ------ | ------ |
| <a id="__usai"></a> `__usai` | `readonly` | `"response"` |
| <a id="status"></a> `status` | `readonly` | `number` |
| <a id="headers"></a> `headers` | `readonly` | `Record`\<`string`, `string`\> |
| <a id="body"></a> `body` | `readonly` | `T` |
