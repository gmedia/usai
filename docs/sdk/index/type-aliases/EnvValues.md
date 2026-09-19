[@sakaladev/usai](../../README.md) / [index](../README.md) / EnvValues

# Type Alias: EnvValues\<S *extends* `Record`\<`string`, [`EnvField`](../interfaces/EnvField.md)\<`unknown`\>\>\>

```ts
type EnvValues<S extends Record<string, EnvField<unknown>>> = { readonly [K in keyof S]: S[K] extends EnvField<infer T> ? T : never };
```

The typed values of a declaration: `EnvValues<typeof spec>`.

## Type Parameters

| Type Parameter |
| ------ |
| `S` *extends* `Record`\<`string`, [`EnvField`](../interfaces/EnvField.md)\<`unknown`\>\> |
