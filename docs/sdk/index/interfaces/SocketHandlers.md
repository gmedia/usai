[@sakaladev/usai](../../README.md) / [index](../README.md) / SocketHandlers

# Interface: SocketHandlers\<I, O\>

The three moments of a connection.

## Type Parameters

| Type Parameter |
| ------ |
| `I` |
| `O` |

## Methods

### open()?

```ts
optional open(ctx: SocketContext<I, O>): unknown;
```

After the upgrade.

#### Parameters

| Parameter | Type |
| ------ | ------ |
| `ctx` | [`SocketContext`](SocketContext.md)\<`I`, `O`\> |

#### Returns

`unknown`

***

### message()?

```ts
optional message(ctx: SocketContext<I, O>): unknown;
```

Once per incoming message, in order.

#### Parameters

| Parameter | Type |
| ------ | ------ |
| `ctx` | [`SocketContext`](SocketContext.md)\<`I`, `O`\> |

#### Returns

`unknown`

***

### close()?

```ts
optional close(ctx: SocketContext<I, O>): unknown;
```

After the connection closed, whoever closed it.

#### Parameters

| Parameter | Type |
| ------ | ------ |
| `ctx` | [`SocketContext`](SocketContext.md)\<`I`, `O`\> |

#### Returns

`unknown`
