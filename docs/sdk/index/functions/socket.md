[@sakaladev/usai](../../README.md) / [index](../README.md) / socket

# Function: socket()

```ts
function socket<I extends AnySchema | undefined = undefined, O extends AnySchema | undefined = undefined, R extends ResourceDeclaration<string, unknown>[] = ResourceDeclaration<string, unknown>[], A extends 
  | AuthDeclaration<unknown, readonly ResourceDeclaration<string, unknown>[]>
  | undefined = undefined>(
   path: string, 
   options: SocketOptions<I, O, R, A>, 
   handlers: SocketHandlers<Out<I>, Out<O>, R, A extends AuthDeclaration<P, readonly ResourceDeclaration<string, unknown>[]> ? P : undefined, A>
): Workload;
```

Declare a WebSocket endpoint: one **connection-bound** world per
connection, from the upgrade to the close. `ctx.state` is the
connection's mutable memory and ends with it; nothing is shared between
connections except through resources. A client disconnect cancels the
world; a draining revision closes the socket with 1012 (service
restart) and `close` runs. `concurrency` bounds open connections.

## Type Parameters

| Type Parameter | Default type |
| ------ | ------ |
| `I` *extends* [`AnySchema`](../type-aliases/AnySchema.md) \| `undefined` | `undefined` |
| `O` *extends* [`AnySchema`](../type-aliases/AnySchema.md) \| `undefined` | `undefined` |
| `R` *extends* [`ResourceDeclaration`](../interfaces/ResourceDeclaration.md)\<`string`, `unknown`\>[] | [`ResourceDeclaration`](../interfaces/ResourceDeclaration.md)\<`string`, `unknown`\>[] |
| `A` *extends* \| [`AuthDeclaration`](../interfaces/AuthDeclaration.md)\<`unknown`, readonly [`ResourceDeclaration`](../interfaces/ResourceDeclaration.md)\<`string`, `unknown`\>[]\> \| `undefined` | `undefined` |

## Parameters

| Parameter | Type |
| ------ | ------ |
| `path` | `string` |
| `options` | [`SocketOptions`](../interfaces/SocketOptions.md)\<`I`, `O`, `R`, `A`\> |
| `handlers` | [`SocketHandlers`](../interfaces/SocketHandlers.md)\<`Out`\<`I`\>, `Out`\<`O`\>, `R`, `A` *extends* [`AuthDeclaration`](../interfaces/AuthDeclaration.md)\<`P`, readonly [`ResourceDeclaration`](../interfaces/ResourceDeclaration.md)\<`string`, `unknown`\>[]\> ? `P` : `undefined`, `A`\> |

## Returns

[`Workload`](../interfaces/Workload.md)

## Example

```ts
export const chat = socket("/chat", { incoming: ChatMessage, outgoing: ChatMessage, resources: [cache] }, {
  open: async (ctx) => { ctx.state.joined = Date.now(); },
  message: async (ctx) => { await ctx.send({ ...ctx.message, echoed: true }); },
  close: async (ctx) => { await ctx.resources.cache.increment("closed"); },
});
```
