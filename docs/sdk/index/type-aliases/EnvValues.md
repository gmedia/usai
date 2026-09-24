[@sakaladev/usai](../../README.md) / [index](../README.md) / EnvValues

# Type Alias: EnvValues\<S *extends* \| [`EnvDeclaration`](../interfaces/EnvDeclaration.md)\<`Record`\<`string`, [`EnvField`](../interfaces/EnvField.md)\<`unknown`\>\>\> \| `Record`\<`string`, [`EnvField`](../interfaces/EnvField.md)\<`unknown`\>\>\>

```ts
type EnvValues<S extends 
  | EnvDeclaration<Record<string, EnvField<unknown>>>
  | Record<string, EnvField<unknown>>> = { readonly [K in keyof EnvFieldsOf<S>]: EnvFieldsOf<S>[K] extends EnvField<infer T> ? T : never };
```

The typed values of a declaration: `EnvValues<typeof spec>`, where
`spec` is what `env({...})` returned (a bare field map works too).

## Type Parameters

| Type Parameter |
| ------ |
| `S` *extends* \| [`EnvDeclaration`](../interfaces/EnvDeclaration.md)\<`Record`\<`string`, [`EnvField`](../interfaces/EnvField.md)\<`unknown`\>\>\> \| `Record`\<`string`, [`EnvField`](../interfaces/EnvField.md)\<`unknown`\>\> |
