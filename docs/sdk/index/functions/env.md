[@sakaladev/usai](../../README.md) / [index](../README.md) / env

# Function: env()

```ts
function env<S extends Record<string, EnvField<unknown>>>(fields: S): EnvDeclaration<S>;
```

Declare what the application needs from its environment. Values are
read by the host when a revision **activates** — a missing required
variable or an unparsable value fails activation, never the first
request — and reach handlers as `ctx.env`, parsed. Variables a resource
names (`DATABASE_URL`, `baseUrlEnv`) are required by that resource and
need no declaration here. `usai run` never reads `.env`; `usai dev` does.

## Type Parameters

| Type Parameter |
| ------ |
| `S` *extends* `Record`\<`string`, [`EnvField`](../interfaces/EnvField.md)\<`unknown`\>\> |

## Parameters

| Parameter | Type |
| ------ | ------ |
| `fields` | `S` |

## Returns

[`EnvDeclaration`](../interfaces/EnvDeclaration.md)\<`S`\>

## Example

```ts
export default defineApp({
  env: env({
    APP_ENV: env.enum(["dev", "prod"]),
    SESSION_TTL_HOURS: env.optional(env.int()),
    WEBHOOK_SECRET: env.secret(),
  }),
});
// in a handler
const ttl = (ctx.env.SESSION_TTL_HOURS as number | undefined) ?? 24;
```
