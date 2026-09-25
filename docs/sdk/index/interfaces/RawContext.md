[@sakaladev/usai](../../README.md) / [index](../README.md) / RawContext

# Interface: RawContext\<R = [`ResourceDeclaration`](ResourceDeclaration.md)[], A = `undefined`\>

The context of a raw request (`http.raw`): no contracts, the exact
bytes on `request`. Read the body once.

## Extends

- [`BaseContext`](BaseContext.md)

## Type Parameters

| Type Parameter | Default type |
| ------ | ------ |
| `R` | [`ResourceDeclaration`](ResourceDeclaration.md)[] |
| `A` | `undefined` |

## Properties

| Property | Modifier | Type | Description | Overrides | Inherited from |
| ------ | ------ | ------ | ------ | ------ | ------ |
| <a id="requestid"></a> `requestId` | `readonly` | `string` | See [HttpContext.requestId](HttpContext.md#requestid). | - | - |
| <a id="method"></a> `method` | `readonly` | [`Method`](../type-aliases/Method.md) | - | - | - |
| <a id="path"></a> `path` | `readonly` | `string` | - | - | - |
| <a id="url"></a> `url` | `readonly` | `string` | - | - | - |
| <a id="params"></a> `params` | `readonly` | `Record`\<`string`, `string`\> | - | - | - |
| <a id="query"></a> `query` | `readonly` | `Record`\<`string`, `string` \| `string`[]\> | - | - | - |
| <a id="headers"></a> `headers` | `readonly` | `Record`\<`string`, `string`\> | - | - | - |
| <a id="auth"></a> `auth` | `readonly` | `A` *extends* [`AuthDeclaration`](AuthDeclaration.md)\<`P`, readonly [`ResourceDeclaration`](ResourceDeclaration.md)\<`string`, `unknown`\>[]\> ? `P` : `undefined` | The principal the `auth` declaration resolved; `undefined` without one. A raw endpoint could always declare `auth:`, the world always ran the resolver, and a 401 always stopped the request — but the context type said nothing, so the principal a signed-webhook endpoint had just proved was unreachable without a cast. | - | - |
| <a id="resources"></a> `resources` | `readonly` | [`ResourcesOf`](../type-aliases/ResourcesOf.md)\<`R`\> & `AuthResourcesOf`\<`A`\> | The declared resources, typed by name from `resources: [...]`, plus the ones the auth scheme leases. | [`BaseContext`](BaseContext.md).[`resources`](BaseContext.md#resources) | - |
| <a id="request"></a> `request` | `readonly` | [`RawRequestBody`](RawRequestBody.md) | The request body, read once in one of three forms. | - | - |
| <a id="tasks"></a> `tasks` | `readonly` | [`TaskHandle`](TaskHandle.md) | Start tasks: owned (`invoke`) or transferred (`dispatch`). | - | [`BaseContext`](BaseContext.md).[`tasks`](BaseContext.md#tasks) |
| <a id="queue"></a> `queue` | `readonly` | [`QueueHandle`](QueueHandle.md) | Publish to a queue topic; durable once the insert commits. | - | [`BaseContext`](BaseContext.md).[`queue`](BaseContext.md#queue) |
| <a id="signal"></a> `signal` | `readonly` | [`UsaiAbortSignal`](UsaiAbortSignal.md) | Aborts when this world is cancelled. | - | [`BaseContext`](BaseContext.md).[`signal`](BaseContext.md#signal) |
| <a id="env"></a> `env` | `readonly` | `Record`\<`string`, [`EnvValue`](../type-aliases/EnvValue.md)\> | The declared environment, parsed: `env.int()` gives a number, `env.bool()` a boolean, `env.list()` an array, `env.optional(...)` may be undefined. A module's declaration is resolved here too. The static type is the union of those; narrow per key, or type it once with `const e = ctx.env as EnvValues<typeof spec>` (the context does not carry the declaration's type). | - | [`BaseContext`](BaseContext.md).[`env`](BaseContext.md#env) |
| <a id="log"></a> `log` | `readonly` | `Pick`\<[`ConsoleLike`](ConsoleLike.md), `"log"` \| `"debug"` \| `"info"` \| `"warn"` \| `"error"`\> | Structured logging; lines carry the workload, world and request ids and reach the runtime's log (`target: "app"`). A trailing plain object is structured `fields` (`ctx.log.info("paid", { invoiceId })`), the rest is the message. `console.*` is the same — including `log`, which the guest emits at `info` and the type used to omit, so the one call everyone reaches for first was a compile error against a context whose `log` *is* `console`. | - | [`BaseContext`](BaseContext.md).[`log`](BaseContext.md#log) |

## Methods

### sleep()

```ts
sleep(duration: string | number): Promise<void>;
```

A timer owned by this world (`"500ms"`, `"2s"`, or milliseconds). It
resolves early when the world is asked to stop, so a service loop can
`await ctx.sleep("1s")` and then check `ctx.signal.aborted`.

#### Parameters

| Parameter | Type |
| ------ | ------ |
| `duration` | `string` \| `number` |

#### Returns

`Promise`\<`void`\>

#### Inherited from

[`BaseContext`](BaseContext.md).[`sleep`](BaseContext.md#sleep)
