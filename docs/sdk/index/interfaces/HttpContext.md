[@sakaladev/usai](../../README.md) / [index](../README.md) / HttpContext

# Interface: HttpContext\<O *extends* [`HttpOptions`](HttpOptions.md) = [`HttpOptions`](HttpOptions.md)\>

The context of one HTTP request, typed from the declared contracts:
`params`, `query`, `headers` and `body` carry the schemas' output types
(already validated at the boundary, before this world existed), `auth`
carries the principal the auth declaration resolved. Everything on
[BaseContext](BaseContext.md) is available too. The world lives for this request
only; nothing here survives the response.

## Extends

- [`BaseContext`](BaseContext.md)

## Type Parameters

| Type Parameter | Default type |
| ------ | ------ |
| `O` *extends* [`HttpOptions`](HttpOptions.md) | [`HttpOptions`](HttpOptions.md) |

## Properties

| Property | Modifier | Type | Description | Overrides | Inherited from |
| ------ | ------ | ------ | ------ | ------ | ------ |
| <a id="requestid"></a> `requestId` | `readonly` | `string` | The request's id: the client's `x-request-id` when it sent a sane one, minted by the runtime otherwise. On the response as `x-request-id`, on every log line this world writes, and on every `httpClient` call it makes. | - | - |
| <a id="method"></a> `method` | `readonly` | [`Method`](../type-aliases/Method.md) | - | - | - |
| <a id="path"></a> `path` | `readonly` | `string` | The matched path, as requested. | - | - |
| <a id="url"></a> `url` | `readonly` | `string` | Path plus query string. | - | - |
| <a id="params"></a> `params` | `readonly` | `OutputOf`\<`O`\[`"params"`\], `Record`\<`string`, `string`\>\> | Path parameters, validated against `params` (strings when undeclared). | - | - |
| <a id="query"></a> `query` | `readonly` | `OutputOf`\<`O`\[`"query"`\], `Record`\<`string`, `string` \| `string`[]\>\> | Query, validated against `query` (strings or arrays when undeclared). | - | - |
| <a id="headers"></a> `headers` | `readonly` | `OutputOf`\<`O`\[`"headers"`\], `Record`\<`string`, `string`\>\> | Headers (lower-cased names), validated against `headers`. | - | - |
| <a id="body"></a> `body` | `readonly` | `OutputOf`\<`O`\[`"body"`\], `unknown`\> | JSON body, validated against `body` (`unknown` when undeclared). | - | - |
| <a id="auth"></a> `auth` | `readonly` | `O`\[`"auth"`\] *extends* [`AuthDeclaration`](AuthDeclaration.md)\<`P`, readonly [`ResourceDeclaration`](ResourceDeclaration.md)\<`string`, `unknown`\>[]\> ? `P` : `undefined` | The principal the `auth` declaration resolved; `undefined` without one. | - | - |
| <a id="resources"></a> `resources` | `readonly` | [`ResourcesOf`](../type-aliases/ResourcesOf.md)\<`O`\[`"resources"`\]\> & `AuthResourcesOf`\<`O`\[`"auth"`\]\> | The declared resources, typed by name from `resources: [...]`. | [`BaseContext`](BaseContext.md).[`resources`](BaseContext.md#resources) | - |
| <a id="tasks"></a> `tasks` | `readonly` | [`TaskHandle`](TaskHandle.md) | Start tasks: owned (`invoke`) or transferred (`dispatch`). | - | [`BaseContext`](BaseContext.md).[`tasks`](BaseContext.md#tasks) |
| <a id="queue"></a> `queue` | `readonly` | [`QueueHandle`](QueueHandle.md) | Publish to a queue topic; durable once the insert commits. | - | [`BaseContext`](BaseContext.md).[`queue`](BaseContext.md#queue) |
| <a id="signal"></a> `signal` | `readonly` | [`UsaiAbortSignal`](UsaiAbortSignal.md) | Aborts when this world is cancelled. | - | [`BaseContext`](BaseContext.md).[`signal`](BaseContext.md#signal) |
| <a id="env"></a> `env` | `readonly` | `Record`\<`string`, `string` \| `number` \| `boolean` \| `undefined`\> | The declared environment, parsed: `env.int()` gives a number, `env.bool()` a boolean, `env.optional(...)` may be undefined. The static type is the union of those; narrow per key, or type it once with `const e = ctx.env as EnvValues<typeof spec>` (the context does not carry the declaration's type). | - | [`BaseContext`](BaseContext.md).[`env`](BaseContext.md#env) |
| <a id="log"></a> `log` | `readonly` | `Pick`\<[`ConsoleLike`](ConsoleLike.md), `"debug"` \| `"info"` \| `"warn"` \| `"error"`\> | Structured logging; lines carry the workload, world and request ids and reach the runtime's log (`target: "app"`). A trailing plain object is structured `fields` (`ctx.log.info("paid", { invoiceId })`), the rest is the message. `console.*` is the same. | - | [`BaseContext`](BaseContext.md).[`log`](BaseContext.md#log) |

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
