[@sakaladev/usai](../../README.md) / [index](../README.md) / EnvValue

# Type Alias: EnvValue

```ts
type EnvValue = 
  | string
  | number
  | boolean
  | readonly (string | number | boolean)[]
  | undefined;
```

What a parsed environment value can be: the scalars, an `env.list`'s
array of them, or `undefined` for an absent optional.
