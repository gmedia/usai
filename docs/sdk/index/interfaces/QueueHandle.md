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
  database?: PostgresDeclaration<string>;
}
): Promise<{
  id: string;
}>;
```

Enqueue a message for the topic's consumer. Resolves with the
message id once the row is durable in the queue's database (the
consumer's `database`, by default the application's first `postgres`
resource — the publishing workload need not declare it); processing
happens later, in the consumer's own world, at least once. The message
must satisfy the consumer's `message` schema or it is dead-lettered on
arrival, so adding a new event means extending that schema first.
Declare the edge with [publishes](../functions/publishes.md) so the reference links the two.
`delayMs` holds the message back; `database` targets another queue.

#### Parameters

| Parameter | Type |
| ------ | ------ |
| `topic` | `string` |
| `message` | `unknown` |
| `options?` | \{ `delayMs?`: `number`; `database?`: [`PostgresDeclaration`](PostgresDeclaration.md)\<`string`\>; \} |
| `options.delayMs?` | `number` |
| `options.database?` | [`PostgresDeclaration`](PostgresDeclaration.md)\<`string`\> |

#### Returns

`Promise`\<\{
  `id`: `string`;
\}\>
