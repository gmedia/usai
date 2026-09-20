[@sakaladev/usai](../../README.md) / [index](../README.md) / TokenClaims

# Type Alias: TokenClaims\<T\>

```ts
type TokenClaims<T> = T & {
  iat: number;
  exp: number;
};
```

What `tokens.verify` returns: the payload as signed, plus the claims
`sign` added.

## Type Declaration

| Name | Type | Description |
| ------ | ------ | ------ |
| `iat` | `number` | Unix seconds the token was issued. |
| `exp` | `number` | Unix seconds the token expires (exclusive). |

## Type Parameters

| Type Parameter |
| ------ |
| `T` |
