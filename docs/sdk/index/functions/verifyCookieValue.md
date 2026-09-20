[@sakaladev/usai](../../README.md) / [index](../README.md) / verifyCookieValue

# Function: verifyCookieValue()

```ts
function verifyCookieValue(signed: string | null | undefined, ...secrets: string[]): Promise<string | null>;
```

The value behind a `signCookieValue` result, or `null` when the
signature does not match any of the secrets (constant-time compare per
secret).

## Parameters

| Parameter | Type |
| ------ | ------ |
| `signed` | `string` \| `null` \| `undefined` |
| ...`secrets` | `string`[] |

## Returns

`Promise`\<`string` \| `null`\>
