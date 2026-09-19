[@sakaladev/usai](../../README.md) / [index](../README.md) / SocketHandlers

# Interface: SocketHandlers\<I, O, R = [`ResourceDeclaration`](ResourceDeclaration.md)[]\>

The three moments of a connection.

## Type Parameters

| Type Parameter | Default type |
| ------ | ------ |
| `I` | - |
| `O` | - |
| `R` | [`ResourceDeclaration`](ResourceDeclaration.md)[] |

## Methods

### open()?

```ts
optional open(ctx: SocketContext<I, O, R>): unknown;
```

After the upgrade.

#### Parameters

| Parameter | Type |
| ------ | ------ |
| `ctx` | [`SocketContext`](SocketContext.md)\<`I`, `O`, `R`\> |

#### Returns

`unknown`

***

### message()?

```ts
optional message(ctx: SocketContext<I, O, R>): unknown;
```

Once per incoming message, in order.

#### Parameters

| Parameter | Type |
| ------ | ------ |
| `ctx` | [`SocketContext`](SocketContext.md)\<`I`, `O`, `R`\> |

#### Returns

`unknown`

***

### close()?

```ts
optional close(ctx: SocketContext<I, O, R>): unknown;
```

After the connection closed, whoever closed it.

#### Parameters

| Parameter | Type |
| ------ | ------ |
| `ctx` | [`SocketContext`](SocketContext.md)\<`I`, `O`, `R`\> |

#### Returns

`unknown`
