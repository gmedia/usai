[@sakaladev/usai](../../README.md) / [index](../README.md) / ConsumeOptions

# Interface: ConsumeOptions\<M *extends* [`AnySchema`](../type-aliases/AnySchema.md) \| `undefined`, R *extends* [`ResourceDeclaration`](ResourceDeclaration.md)[] = [`ResourceDeclaration`](ResourceDeclaration.md)[]\>

Options for `queue.consume`.

## Extends

- `ConsumerPolicies`

## Type Parameters

| Type Parameter | Default type |
| ------ | ------ |
| `M` *extends* [`AnySchema`](../type-aliases/AnySchema.md) \| `undefined` | - |
| `R` *extends* [`ResourceDeclaration`](ResourceDeclaration.md)[] | [`ResourceDeclaration`](ResourceDeclaration.md)[] |

## Properties

| Property | Type | Description | Inherited from |
| ------ | ------ | ------ | ------ |
| <a id="timeout"></a> `timeout?` | `string` \| `number` | Per-invocation deadline (`"5s"`, `"500ms"`, or milliseconds). For **finite** work the world is cancelled when it passes: an HTTP caller gets 504, an `invoke` rejects with `deadline_exceeded`, a queue message counts as a failed attempt. Undeclared, finite work gets the runtime default of 30 s. For **connection-bound and persistent** work — a stream, a socket, a service — there is **no default**, because one that ended after 30 s would be useless. A deadline you declare is honoured, and it **stops** the world rather than cancelling it: `ctx.signal` aborts, a pending `ctx.sleep` returns, the handler can finish what it is doing, and a socket's connection is closed so its `close` handler runs. It has one second to unwind and is then cancelled, so a handler that never checks `ctx.signal` still stops near the bound you declared. For a stream the client keeps the `200` it already has and the body stops there — with no trailer and no error, so end an export with a sentinel the reader requires. | [`WorkloadPolicies`](WorkloadPolicies.md).[`timeout`](WorkloadPolicies.md#timeout) |
| <a id="description"></a> `description?` | `string` | A paragraph for the reference. | - |
| <a id="message"></a> `message?` | `M` | Schema for the message; validated before the message's world exists. A message that fails validation is dead-lettered, not retried. | - |
| <a id="concurrency"></a> `concurrency?` | `number` | Messages processed at once by this consumer. Default 1. | - |
| <a id="retry"></a> `retry?` | [`RetryOptions`](RetryOptions.md) | Delivery is at-least-once; declare retry to accept re-delivery. | - |
| <a id="database"></a> `database?` | [`PostgresDeclaration`](PostgresDeclaration.md)\<`string`\> | PostgreSQL resource holding the `usai_queue` table. Default: the first `postgres` resource declared in the application (declaration order: app-level resources, then modules in order). Publishers use the same default, so one database serves every topic unless both sides name another. | - |
| <a id="errors"></a> `errors?` | [`DeclaredError`](DeclaredError.md)[] | - | - |
| <a id="resources"></a> `resources?` | `R` | Resources the message's world leases; `ctx.resources` is typed from this list. The queue's own database need not be listed. | - |
