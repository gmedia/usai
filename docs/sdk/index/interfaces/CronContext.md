[@sakaladev/usai](../../README.md) / [index](../README.md) / CronContext

# Interface: CronContext\<R = [`ResourceDeclaration`](ResourceDeclaration.md)[]\>

The context of one cron tick: `scheduledAt` (ISO 8601, the tick's
nominal time) plus [BaseContext](BaseContext.md).

## Extends

- [`BaseContext`](BaseContext.md)

## Type Parameters

| Type Parameter | Default type |
| ------ | ------ |
| `R` | [`ResourceDeclaration`](ResourceDeclaration.md)[] |

## Properties

| Property | Modifier | Type | Description | Overrides | Inherited from |
| ------ | ------ | ------ | ------ | ------ | ------ |
| <a id="tasks"></a> `tasks` | `readonly` | [`TaskHandle`](TaskHandle.md) | Start tasks: owned (`invoke`) or transferred (`dispatch`). | - | [`BaseContext`](BaseContext.md).[`tasks`](BaseContext.md#tasks) |
| <a id="queue"></a> `queue` | `readonly` | [`QueueHandle`](QueueHandle.md) | Publish to a queue topic; durable once the insert commits. | - | [`BaseContext`](BaseContext.md).[`queue`](BaseContext.md#queue) |
| <a id="signal"></a> `signal` | `readonly` | [`UsaiAbortSignal`](UsaiAbortSignal.md) | Aborts when this world is cancelled. | - | [`BaseContext`](BaseContext.md).[`signal`](BaseContext.md#signal) |
| <a id="env"></a> `env` | `readonly` | `Record`\<`string`, `string` \| `number` \| `boolean` \| `undefined`\> | The declared environment, parsed: `env.int()` gives a number, `env.bool()` a boolean, `env.optional(...)` may be undefined. The static type is the union of those; narrow per key, or type it once with `const e = ctx.env as EnvValues<typeof spec>` (the context does not carry the declaration's type). | - | [`BaseContext`](BaseContext.md).[`env`](BaseContext.md#env) |
| <a id="log"></a> `log` | `readonly` | `Pick`\<[`ConsoleLike`](ConsoleLike.md), `"log"` \| `"debug"` \| `"info"` \| `"warn"` \| `"error"`\> | Structured logging; lines carry the workload, world and request ids and reach the runtime's log (`target: "app"`). A trailing plain object is structured `fields` (`ctx.log.info("paid", { invoiceId })`), the rest is the message. `console.*` is the same — including `log`, which the guest emits at `info` and the type used to omit, so the one call everyone reaches for first was a compile error against a context whose `log` *is* `console`. | - | [`BaseContext`](BaseContext.md).[`log`](BaseContext.md#log) |
| <a id="scheduledat"></a> `scheduledAt` | `readonly` | `string` | - | - | - |
| <a id="resources"></a> `resources` | `readonly` | [`ResourcesOf`](../type-aliases/ResourcesOf.md)\<`R`\> | The declared resources by name, as their in-world handles ([PostgresHandle](PostgresHandle.md), [CacheLocalHandle](CacheLocalHandle.md), [HttpClientHandle](HttpClientHandle.md)). Reading an undeclared name throws `resource_not_declared` with the fix. | [`BaseContext`](BaseContext.md).[`resources`](BaseContext.md#resources) | - |

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
