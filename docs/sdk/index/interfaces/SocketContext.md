[@sakaladev/usai](../../README.md) / [index](../README.md) / SocketContext

# Interface: SocketContext\<Incoming, Outgoing, R = [`ResourceDeclaration`](ResourceDeclaration.md)[], A = `unknown`\>

The context of a WebSocket connection, shared by `open`, `message` and
`close`: request facts, `send`/`close`, connection-local `state`, and
in `message` the validated incoming `message`.

## Extends

- [`BaseContext`](BaseContext.md)

## Type Parameters

| Type Parameter | Default type |
| ------ | ------ |
| `Incoming` | - |
| `Outgoing` | - |
| `R` | [`ResourceDeclaration`](ResourceDeclaration.md)[] |
| `A` | `unknown` |

## Properties

| Property | Modifier | Type | Description | Overrides | Inherited from |
| ------ | ------ | ------ | ------ | ------ | ------ |
| <a id="resources"></a> `resources` | `readonly` | [`ResourcesOf`](../type-aliases/ResourcesOf.md)\<`R`\> | The declared resources by name, as their in-world handles ([PostgresHandle](PostgresHandle.md), [CacheLocalHandle](CacheLocalHandle.md), [HttpClientHandle](HttpClientHandle.md)). Reading an undeclared name throws `resource_not_declared` with the fix. | [`BaseContext`](BaseContext.md).[`resources`](BaseContext.md#resources) | - |
| <a id="path"></a> `path` | `readonly` | `string` | - | - | - |
| <a id="url"></a> `url` | `readonly` | `string` | - | - | - |
| <a id="params"></a> `params` | `readonly` | `Record`\<`string`, `string`\> | - | - | - |
| <a id="query"></a> `query` | `readonly` | `Record`\<`string`, `string` \| `string`[]\> | - | - | - |
| <a id="headers"></a> `headers` | `readonly` | `Record`\<`string`, `string`\> | - | - | - |
| <a id="auth"></a> `auth` | `readonly` | `A` | The principal the `auth` declaration resolved; `undefined` without one. | - | - |
| <a id="state"></a> `state` | `readonly` | `Record`\<`string`, `unknown`\> | Connection-local mutable state: survives messages, ends with the connection. | - | - |
| <a id="message"></a> `message` | `readonly` | `Incoming` | The current message (in the `message` handler). | - | - |
| <a id="closeinfo"></a> `closeInfo` | `readonly` | \| \{ `code`: `number` \| `null`; `reason`: `string`; \} \| `null` | Why the connection closed (in the `close` handler). | - | - |
| <a id="tasks"></a> `tasks` | `readonly` | [`TaskHandle`](TaskHandle.md) | Start tasks: owned (`invoke`) or transferred (`dispatch`). | - | [`BaseContext`](BaseContext.md).[`tasks`](BaseContext.md#tasks) |
| <a id="queue"></a> `queue` | `readonly` | [`QueueHandle`](QueueHandle.md) | Publish to a queue topic; durable once the insert commits. | - | [`BaseContext`](BaseContext.md).[`queue`](BaseContext.md#queue) |
| <a id="signal"></a> `signal` | `readonly` | [`UsaiAbortSignal`](UsaiAbortSignal.md) | Aborts when this world is cancelled. | - | [`BaseContext`](BaseContext.md).[`signal`](BaseContext.md#signal) |
| <a id="env"></a> `env` | `readonly` | `Record`\<`string`, `string` \| `number` \| `boolean` \| `undefined`\> | The declared environment, parsed: `env.int()` gives a number, `env.bool()` a boolean, `env.optional(...)` may be undefined. The static type is the union of those; narrow per key, or type it once with `const e = ctx.env as EnvValues<typeof spec>` (the context does not carry the declaration's type). | - | [`BaseContext`](BaseContext.md).[`env`](BaseContext.md#env) |
| <a id="log"></a> `log` | `readonly` | `Pick`\<[`ConsoleLike`](ConsoleLike.md), `"debug"` \| `"info"` \| `"warn"` \| `"error"`\> | Structured logging; lines carry the workload and world ids and reach the runtime's log (`target: "app"`). `console.*` is the same. | - | [`BaseContext`](BaseContext.md).[`log`](BaseContext.md#log) |

## Methods

### send()

```ts
send(message: Outgoing): Promise<void>;
```

Send one message (validated against `outgoing` when declared).

#### Parameters

| Parameter | Type |
| ------ | ------ |
| `message` | `Outgoing` |

#### Returns

`Promise`\<`void`\>

***

### close()

```ts
close(reason?: string): Promise<void>;
```

Close the connection; the world ends after `close` ran.

#### Parameters

| Parameter | Type |
| ------ | ------ |
| `reason?` | `string` |

#### Returns

`Promise`\<`void`\>

***

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
