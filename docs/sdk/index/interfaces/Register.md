[@sakaladev/usai](../../README.md) / [index](../README.md) / Register

# Interface: Register

Where an application registers the type of its environment, once, so
`ctx.env` is typed in every handler and auth resolver without a cast:

```ts
export const appEnv = env({ API_TOKEN: env.secret(), LEASE_MS: env.optional(env.int()) });
declare module "@sakaladev/usai" {
  interface Register { env: EnvValues<typeof appEnv> }
}
```

After that `ctx.env.API_TOKEN` is a `string`, `ctx.env.LEASE_MS` a
`number | undefined`, and a name nobody declared is a compile error.
Modules with their own declarations add theirs to the same type
(`EnvValues<typeof appEnv> & EnvValues<typeof billingEnv>`). Without a
registration `ctx.env` stays `Record<string, EnvValue>`.

Only the type is registered: what the runtime checks at activation is
still the declaration passed to `defineApp`/`defineModule`, so register
the same object you pass there.
