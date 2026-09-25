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
  tx?: SqlExecutor;
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

**`tx` makes the message part of your transaction** — a transactional
outbox. Pass the executor `sql.transaction(async (tx) => …)` gave you
and the `usai_queue` row commits or rolls back with the writes beside
it, so "store the detection and enqueue its evaluation" is one fact
rather than two. Without it, `publish` commits on its own connection:
a request that fails after publishing has enqueued work for something
that does not exist, and a retried request enqueues it again — which
is why a consumer has to be idempotent either way, but not why it
should have to clean up after work that never happened.

The transaction must be on the **same** resource as the queue, since
one transaction is one connection to one database; a mismatch is
refused rather than silently committing on its own.

```ts
await ctx.resources.db.transaction(async (tx) => {
  const { id } = await tx.one("insert into detections (...) values (...) returning id");
  await ctx.queue.publish("evaluate", { detectionId: id }, { tx });
});
```

#### Parameters

| Parameter | Type |
| ------ | ------ |
| `topic` | `string` |
| `message` | `unknown` |
| `options?` | \{ `delayMs?`: `number`; `database?`: [`PostgresDeclaration`](PostgresDeclaration.md)\<`string`\>; `tx?`: [`SqlExecutor`](SqlExecutor.md); \} |
| `options.delayMs?` | `number` |
| `options.database?` | [`PostgresDeclaration`](PostgresDeclaration.md)\<`string`\> |
| `options.tx?` | [`SqlExecutor`](SqlExecutor.md) |

#### Returns

`Promise`\<\{
  `id`: `string`;
\}\>
