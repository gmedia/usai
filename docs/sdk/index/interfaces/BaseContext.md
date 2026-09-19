[@sakaladev/usai](../../README.md) / [index](../README.md) / BaseContext

# Interface: BaseContext

What every handler receives, whatever the workload kind. Everything
asynchronous here is a host operation **owned by this world**: it is
cancelled when the world is, and a finite world may not end while one is
still pending (that is a lifecycle error with a diagnostic, not a leak).
Nothing on the context survives the world.

## Extended by

- [`HttpContext`](HttpContext.md)
- [`RawContext`](RawContext.md)
- [`SeederContext`](SeederContext.md)
- [`TaskContext`](TaskContext.md)
- [`CronContext`](CronContext.md)
- [`CommandContext`](CommandContext.md)
- [`ServiceContext`](ServiceContext.md)
- [`StreamContext`](StreamContext.md)
- [`SocketContext`](SocketContext.md)
- [`QueueContext`](QueueContext.md)

## Properties

| Property | Modifier | Type | Description |
| ------ | ------ | ------ | ------ |
| <a id="resources"></a> `resources` | `readonly` | `Record`\<`string`, `unknown`\> | The declared resources by name, as their in-world handles ([PostgresHandle](PostgresHandle.md), [CacheLocalHandle](CacheLocalHandle.md), [HttpClientHandle](HttpClientHandle.md)). Reading an undeclared name throws `resource_not_declared` with the fix. |
| <a id="tasks"></a> `tasks` | `readonly` | [`TaskHandle`](TaskHandle.md) | Start tasks: owned (`invoke`) or transferred (`dispatch`). |
| <a id="queue"></a> `queue` | `readonly` | [`QueueHandle`](QueueHandle.md) | Publish to a queue topic; durable once the insert commits. |
| <a id="signal"></a> `signal` | `readonly` | [`UsaiAbortSignal`](UsaiAbortSignal.md) | Aborts when this world is cancelled. |
| <a id="env"></a> `env` | `readonly` | `Record`\<`string`, `string` \| `number` \| `boolean` \| `undefined`\> | The declared environment, parsed: `env.int()` gives a number, `env.bool()` a boolean, `env.optional(...)` may be undefined. The static type is the union of those; narrow per key, or type it once with `const e = ctx.env as EnvValues<typeof spec>` (the context does not carry the declaration's type). |
| <a id="log"></a> `log` | `readonly` | `Pick`\<[`ConsoleLike`](ConsoleLike.md), `"debug"` \| `"info"` \| `"warn"` \| `"error"`\> | Structured logging; lines carry the workload and world ids and reach the runtime's log (`target: "app"`). `console.*` is the same. |

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
