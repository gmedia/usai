[@sakaladev/usai](../../README.md) / [index](../README.md) / AppEnv

# Type Alias: AppEnv

```ts
type AppEnv = Register extends {
  env: infer E;
} ? E : Record<string, EnvValue>;
```

The type of `ctx.env`: the registered environment ([Register](../interfaces/Register.md)), or
`Record<string, EnvValue>` when none is registered.
