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
need no declaration here. Everything else works the other way round: a
variable **not** declared here is `undefined` in `ctx.env` even when it is
set in the process environment — the contract is the whole of what a world
can see. A module states its own with `defineModule({ env })`, which is
merged into this one, so its consumers do not have to mirror it.
`usai run` never reads `.env`; `usai dev` does.

Field constructors (all required unless wrapped in `env.optional`):

| Constructor | `ctx.env.X` | Accepts |
|---|---|---|
| `env.string()` | `string` | any text |
| `env.url()` | `string` | a URL, kept as text |
| `env.secret()` | `string` | any text; never printed by `inspect` |
| `env.int()` | `number` | an integer |
| `env.bool()` | `boolean` | `true`/`1`, `false`/`0` |
| `env.enum([...])` | the union | one of the listed values |
| `env.optional(field)` | `T \| undefined` | absent or empty → `undefined` |

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
