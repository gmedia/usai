[@sakaladev/usai](../../README.md) / [index](../README.md) / verifyToken

# Function: verifyToken()

```ts
function verifyToken<T extends Record<string, unknown> = Record<string, unknown>>(
   token: string | null | undefined, 
   secrets: string | readonly string[], 
   options?: {
  now?: number;
}
): Promise<TokenClaims<T> | null>;
```

The claims of a token `signToken` produced, or `null` when the token is
malformed, expired, or signed with none of the secrets (pass an array to
rotate: verify against old and new, sign with the new). Comparison is
constant-time per secret. Nothing about *why* it failed is returned — a
client gets `401 unauthorized` either way, and the reason is not its
business.

## Type Parameters

| Type Parameter | Default type |
| ------ | ------ |
| `T` *extends* `Record`\<`string`, `unknown`\> | `Record`\<`string`, `unknown`\> |

## Parameters

| Parameter | Type |
| ------ | ------ |
| `token` | `string` \| `null` \| `undefined` |
| `secrets` | `string` \| readonly `string`[] |
| `options` | \{ `now?`: `number`; \} |
| `options.now?` | `number` |

## Returns

`Promise`\<[`TokenClaims`](../type-aliases/TokenClaims.md)\<`T`\> \| `null`\>

## Example

```ts
const userBearer = auth.bearer({
  name: "user",
  resolve: async (ctx, token) => {
    const claims = await tokens.verify<{ sub: string }>(token, String(ctx.env.ACCESS_TOKEN_SECRET));
    if (!claims) throw errors.unauthorized();
    return { userId: claims.sub };
  },
});
```
