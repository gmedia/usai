[@sakaladev/usai](../../README.md) / [index](../README.md) / QueueHandle

# Interface: QueueHandle

`ctx.queue` in every world.

## Methods

### publish()

```ts
publish(
   topic: string, 
   message: unknown, 
   options?: {
  delayMs?: number;
  database?: PostgresDeclaration;
}
): Promise<{
  id: string;
}>;
```

Enqueues a message. Resolves once the insert is durable in the queue's
database; processing happens in its own world later.

#### Parameters

| Parameter | Type |
| ------ | ------ |
| `topic` | `string` |
| `message` | `unknown` |
| `options?` | \{ `delayMs?`: `number`; `database?`: [`PostgresDeclaration`](PostgresDeclaration.md); \} |
| `options.delayMs?` | `number` |
| `options.database?` | [`PostgresDeclaration`](PostgresDeclaration.md) |

#### Returns

`Promise`\<\{
  `id`: `string`;
\}\>
