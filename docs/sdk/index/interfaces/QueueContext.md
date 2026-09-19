[@sakaladev/usai](../../README.md) / [index](../README.md) / QueueContext

# Interface: QueueContext\<M\>

The context of one message delivery: the validated `message`, the
`attempt` number and the message `id`, plus [BaseContext](BaseContext.md).

## Extends

- [`BaseContext`](BaseContext.md)

## Type Parameters

| Type Parameter |
| ------ |
| `M` |

## Properties

| Property | Modifier | Type | Description | Inherited from |
| ------ | ------ | ------ | ------ | ------ |
| <a id="message"></a> `message` | `readonly` | `M` | - | - |
| <a id="attempt"></a> `attempt` | `readonly` | `number` | 1-based attempt number. | - |
| <a id="id"></a> `id` | `readonly` | `string` | The queue's id for this message (the one `publish` returned). | - |
| <a id="resources"></a> `resources` | `readonly` | `Record`\<`string`, `unknown`\> | The declared resources by name, as their in-world handles ([PostgresHandle](PostgresHandle.md), [CacheLocalHandle](CacheLocalHandle.md), [HttpClientHandle](HttpClientHandle.md)). Reading an undeclared name throws `resource_not_declared` with the fix. | [`BaseContext`](BaseContext.md).[`resources`](BaseContext.md#resources) |
| <a id="tasks"></a> `tasks` | `readonly` | [`TaskHandle`](TaskHandle.md) | Start tasks: owned (`invoke`) or transferred (`dispatch`). | [`BaseContext`](BaseContext.md).[`tasks`](BaseContext.md#tasks) |
| <a id="queue"></a> `queue` | `readonly` | [`QueueHandle`](QueueHandle.md) | Publish to a queue topic; durable once the insert commits. | [`BaseContext`](BaseContext.md).[`queue`](BaseContext.md#queue) |
| <a id="signal"></a> `signal` | `readonly` | [`UsaiAbortSignal`](UsaiAbortSignal.md) | Aborts when this world is cancelled. | [`BaseContext`](BaseContext.md).[`signal`](BaseContext.md#signal) |
| <a id="env"></a> `env` | `readonly` | `Record`\<`string`, `string` \| `number` \| `boolean` \| `undefined`\> | The declared environment, typed: `env.int()` gives a number, `env.bool()` a boolean, `env.optional(...)` may be undefined. Narrow per key, or type it once: `const e = ctx.env as EnvValues<typeof spec>`. | [`BaseContext`](BaseContext.md).[`env`](BaseContext.md#env) |
| <a id="log"></a> `log` | `readonly` | `Pick`\<[`ConsoleLike`](ConsoleLike.md), `"debug"` \| `"info"` \| `"warn"` \| `"error"`\> | Structured logging; lines carry the workload and world ids and reach the runtime's log (`target: "app"`). `console.*` is the same. | [`BaseContext`](BaseContext.md).[`log`](BaseContext.md#log) |

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
