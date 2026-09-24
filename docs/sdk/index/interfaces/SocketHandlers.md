[@sakaladev/usai](../../README.md) / [index](../README.md) / SocketHandlers

# Interface: SocketHandlers\<I, O, R = [`ResourceDeclaration`](ResourceDeclaration.md)[], A = `unknown`, D = `undefined`, P = `Record`\<`string`, `string`\>, Q = `Record`\<`string`, `string` \| `string`[]\>\>

The three moments of a connection.

## Type Parameters

| Type Parameter | Default type |
| ------ | ------ |
| `I` | - |
| `O` | - |
| `R` | [`ResourceDeclaration`](ResourceDeclaration.md)[] |
| `A` | `unknown` |
| `D` | `undefined` |
| `P` | `Record`\<`string`, `string`\> |
| `Q` | `Record`\<`string`, `string` \| `string`[]\> |

## Methods

### open()?

```ts
optional open(ctx: SocketContext<I, O, R, A, D, P, Q>): unknown;
```

After the upgrade. Note that **a message is not delivered until `open`
returns**: a socket either pushes or converses, not both (GUIDE §9).

#### Parameters

| Parameter | Type |
| ------ | ------ |
| `ctx` | [`SocketContext`](SocketContext.md)\<`I`, `O`, `R`, `A`, `D`, `P`, `Q`\> |

#### Returns

`unknown`

***

### message()?

```ts
optional message(ctx: SocketContext<I, O, R, A, D, P, Q>): unknown;
```

Once per incoming message, in order.

#### Parameters

| Parameter | Type |
| ------ | ------ |
| `ctx` | [`SocketContext`](SocketContext.md)\<`I`, `O`, `R`, `A`, `D`, `P`, `Q`\> |

#### Returns

`unknown`

***

### close()?

```ts
optional close(ctx: SocketContext<I, O, R, A, D, P, Q>): unknown;
```

After the connection closed, however it closed — including when `open`
itself failed, which is how a push loop normally ends (`ctx.send`
rejects with `client_gone` once the client is gone). This is the only
place to release what the connection held.

#### Parameters

| Parameter | Type |
| ------ | ------ |
| `ctx` | [`SocketContext`](SocketContext.md)\<`I`, `O`, `R`, `A`, `D`, `P`, `Q`\> |

#### Returns

`unknown`
