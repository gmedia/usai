[@sakaladev/usai](../../README.md) / [index](../README.md) / SocketContext

# Interface: SocketContext\<Incoming, Outgoing, R = [`ResourceDeclaration`](ResourceDeclaration.md)[], A = `unknown`, D = `undefined`, P = `Record`\<`string`, `string`\>, Q = `Record`\<`string`, `string` \| `string`[]\>\>

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
| `D` | `undefined` |
| `P` | `Record`\<`string`, `string`\> |
| `Q` | `Record`\<`string`, `string` \| `string`[]\> |

## Properties

| Property | Modifier | Type | Description | Overrides | Inherited from |
| ------ | ------ | ------ | ------ | ------ | ------ |
| <a id="requestid"></a> `requestId` | `readonly` | `string` | The upgrade request's id (`x-request-id`, the client's or minted). | - | - |
| <a id="resources"></a> `resources` | `readonly` | [`ResourcesOf`](../type-aliases/ResourcesOf.md)\<`R`\> & `AuthResourcesOf`\<`D`\> | The declared resources, plus the ones the auth scheme (`D`) leases. | [`BaseContext`](BaseContext.md).[`resources`](BaseContext.md#resources) | - |
| <a id="path"></a> `path` | `readonly` | `string` | - | - | - |
| <a id="url"></a> `url` | `readonly` | `string` | - | - | - |
| <a id="params"></a> `params` | `readonly` | `P` | The path parameters, typed and **validated before the world exists** when `params` is declared (C6, the same as an HTTP route); a plain `Record<string, string>` the handler must check itself otherwise. | - | - |
| <a id="query"></a> `query` | `readonly` | `Q` | The upgrade's query string, typed and validated when `query` is declared. | - | - |
| <a id="headers"></a> `headers` | `readonly` | `Record`\<`string`, `string`\> | - | - | - |
| <a id="auth"></a> `auth` | `readonly` | `A` | The principal the `auth` declaration resolved; `undefined` without one. | - | - |
| <a id="state"></a> `state` | `readonly` | `Record`\<`string`, `unknown`\> | Connection-local mutable state: survives messages, ends with the connection. | - | - |
| <a id="message"></a> `message` | `readonly` | `Incoming` | The current message (in the `message` handler). | - | - |
| <a id="closeinfo"></a> `closeInfo` | `readonly` | \| \{ `code`: `number` \| `null`; `reason`: `string`; \} \| `null` | Why the connection closed (in the `close` handler). | - | - |
| <a id="tasks"></a> `tasks` | `readonly` | [`TaskHandle`](TaskHandle.md) | Start tasks: owned (`invoke`) or transferred (`dispatch`). | - | [`BaseContext`](BaseContext.md).[`tasks`](BaseContext.md#tasks) |
| <a id="queue"></a> `queue` | `readonly` | [`QueueHandle`](QueueHandle.md) | Publish to a queue topic; durable once the insert commits. | - | [`BaseContext`](BaseContext.md).[`queue`](BaseContext.md#queue) |
| <a id="signal"></a> `signal` | `readonly` | [`UsaiAbortSignal`](UsaiAbortSignal.md) | Aborts when this world is cancelled. | - | [`BaseContext`](BaseContext.md).[`signal`](BaseContext.md#signal) |
| <a id="env"></a> `env` | `readonly` | `Record`\<`string`, [`EnvValue`](../type-aliases/EnvValue.md)\> | The declared environment, parsed: `env.int()` gives a number, `env.bool()` a boolean, `env.list()` an array, `env.optional(...)` may be undefined. A module's declaration is resolved here too. The static type is the union of those; narrow per key, or type it once with `const e = ctx.env as EnvValues<typeof spec>` (the context does not carry the declaration's type). | - | [`BaseContext`](BaseContext.md).[`env`](BaseContext.md#env) |
| <a id="log"></a> `log` | `readonly` | `Pick`\<[`ConsoleLike`](ConsoleLike.md), `"log"` \| `"debug"` \| `"info"` \| `"warn"` \| `"error"`\> | Structured logging; lines carry the workload, world and request ids and reach the runtime's log (`target: "app"`). A trailing plain object is structured `fields` (`ctx.log.info("paid", { invoiceId })`), the rest is the message. `console.*` is the same — including `log`, which the guest emits at `info` and the type used to omit, so the one call everyone reaches for first was a compile error against a context whose `log` *is* `console`. | - | [`BaseContext`](BaseContext.md).[`log`](BaseContext.md#log) |

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
