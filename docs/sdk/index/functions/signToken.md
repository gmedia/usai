[@sakaladev/usai](../../README.md) / [index](../README.md) / signToken

# Function: signToken()

```ts
function signToken<T extends Record<string, unknown>>(
   payload: T, 
   secret: string, 
   options: TokenOptions
): Promise<string>;
```

Signs `payload` (a JSON object of your claims — a user id, a role) with
`secret`, adding `iat` and `exp`. The result is `<payload>.<signature>`,
both base64url, opaque to the client, verifiable by any instance that
holds the secret.

## Type Parameters

| Type Parameter |
| ------ |
| `T` *extends* `Record`\<`string`, `unknown`\> |

## Parameters

| Parameter | Type |
| ------ | ------ |
| `payload` | `T` |
| `secret` | `string` |
| `options` | [`TokenOptions`](../interfaces/TokenOptions.md) |

## Returns

`Promise`\<`string`\>

## Example

```ts
const access = await tokens.sign({ sub: user.id, role: user.role }, ctx.env.ACCESS_TOKEN_SECRET, { expiresIn: "15m" });
```
