[@sakaladev/usai](../../README.md) / [index](../README.md) / signCookieValue

# Function: signCookieValue()

```ts
function signCookieValue(value: string, secret: string): Promise<string>;
```

`value.signature` — a value the client can read but not alter. The
signature is HMAC-SHA256 over the value with `secret`, base64url. Rotate
by verifying against several secrets and signing with the newest.

## Parameters

| Parameter | Type |
| ------ | ------ |
| `value` | `string` |
| `secret` | `string` |

## Returns

`Promise`\<`string`\>
