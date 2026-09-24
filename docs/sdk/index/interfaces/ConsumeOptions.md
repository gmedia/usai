[@sakaladev/usai](../../README.md) / [index](../README.md) / ConsumeOptions

# Interface: ConsumeOptions\<M *extends* [`AnySchema`](../type-aliases/AnySchema.md) \| `undefined`, R *extends* [`ResourceDeclaration`](ResourceDeclaration.md)[] = [`ResourceDeclaration`](ResourceDeclaration.md)[]\>

Options for `queue.consume`.

## Extends

- [`WorkloadPolicies`](WorkloadPolicies.md)

## Type Parameters

| Type Parameter | Default type |
| ------ | ------ |
| `M` *extends* [`AnySchema`](../type-aliases/AnySchema.md) \| `undefined` | - |
| `R` *extends* [`ResourceDeclaration`](ResourceDeclaration.md)[] | [`ResourceDeclaration`](ResourceDeclaration.md)[] |

## Properties

| Property | Type | Description | Overrides | Inherited from |
| ------ | ------ | ------ | ------ | ------ |
| <a id="timeout"></a> `timeout?` | `string` \| `number` | Per-invocation deadline (`"5s"`, `"500ms"`, or milliseconds). The world is cancelled when it passes: an HTTP caller gets 504, an `invoke` rejects with `deadline_exceeded`, a queue message counts as a failed attempt, and a **stream** ends — the client keeps the 200 it already has and the body stops there. Undeclared: the runtime default (30 s) for requests, tasks, cron ticks, queue messages, commands, migrations and seeders; **none** for a stream, a socket or a service, which would be useless with one. A deadline you declare is honoured whatever the kind — it is the only way to bound an export. | - | [`WorkloadPolicies`](WorkloadPolicies.md).[`timeout`](WorkloadPolicies.md#timeout) |
| <a id="maxbodybytes"></a> `maxBodyBytes?` | `number` | Request body bound for this route, in bytes. A **cap**, never a raise: the effective bound is the smaller of this and the process's `USAI_MAX_BODY_BYTES` (1 MiB by default), so the operator keeps the ceiling and each route decides how much of it to accept. Declare a small one on ordinary routes and raise the process bound for the one that takes uploads, instead of opening every route to the largest body any of them needs. Above the bound the request is `413 payload_too_large`, decided before a world exists. | - | [`WorkloadPolicies`](WorkloadPolicies.md).[`maxBodyBytes`](WorkloadPolicies.md#maxbodybytes) |
| <a id="description"></a> `description?` | `string` | A paragraph for the reference. | - | - |
| <a id="message"></a> `message?` | `M` | Schema for the message; validated before the message's world exists. A message that fails validation is dead-lettered, not retried. | - | - |
| <a id="concurrency"></a> `concurrency?` | `number` | Messages processed at once by this consumer. Default 1. | [`WorkloadPolicies`](WorkloadPolicies.md).[`concurrency`](WorkloadPolicies.md#concurrency) | - |
| <a id="retry"></a> `retry?` | [`RetryOptions`](RetryOptions.md) | Delivery is at-least-once; declare retry to accept re-delivery. | - | - |
| <a id="database"></a> `database?` | [`PostgresDeclaration`](PostgresDeclaration.md)\<`string`\> | PostgreSQL resource holding the `usai_queue` table. Default: the first `postgres` resource declared in the application (declaration order: app-level resources, then modules in order). Publishers use the same default, so one database serves every topic unless both sides name another. | - | - |
| <a id="errors"></a> `errors?` | [`DeclaredError`](DeclaredError.md)[] | - | - | - |
| <a id="resources"></a> `resources?` | `R` | Resources the message's world leases; `ctx.resources` is typed from this list. The queue's own database need not be listed. | - | - |
