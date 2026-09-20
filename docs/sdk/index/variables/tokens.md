[@sakaladev/usai](../../README.md) / [index](../README.md) / tokens

# Variable: tokens

```ts
const tokens: {
  sign: <T>(payload: T, secret: string, options: TokenOptions) => Promise<string>;
  verify: <T>(token: string | null | undefined, secrets: string | readonly string[], options: {
     now?: number;
  }) => Promise<TokenClaims<T> | null>;
};
```

The token helpers as one object, for `import { tokens } from "@sakaladev/usai"`.

## Type Declaration

## Authentication

| Name | Type | Default value |
| ------ | ------ | ------ |
| <a id="property-sign"></a> `sign()` | \<`T`\>(`payload`: `T`, `secret`: `string`, `options`: [`TokenOptions`](../interfaces/TokenOptions.md)) => `Promise`\<`string`\> | `signToken` |
| <a id="property-verify"></a> `verify()` | \<`T`\>(`token`: `string` \| `null` \| `undefined`, `secrets`: `string` \| readonly `string`[], `options`: \{ `now?`: `number`; \}) => `Promise`\<[`TokenClaims`](../type-aliases/TokenClaims.md)\<`T`\> \| `null`\> | `verifyToken` |
