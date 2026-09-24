[@sakaladev/usai](../../README.md) / [index](../README.md) / SocketOptions

# Interface: SocketOptions\<I *extends* [`AnySchema`](../type-aliases/AnySchema.md) \| `undefined`, O *extends* [`AnySchema`](../type-aliases/AnySchema.md) \| `undefined`, R *extends* [`ResourceDeclaration`](ResourceDeclaration.md)[] = [`ResourceDeclaration`](ResourceDeclaration.md)[], A *extends* [`AuthDeclaration`](AuthDeclaration.md) \| `undefined` = [`AuthDeclaration`](AuthDeclaration.md) \| `undefined`\>

Options for [socket](../functions/socket.md).

## Extends

- [`WorkloadPolicies`](WorkloadPolicies.md)

## Type Parameters

| Type Parameter | Default type |
| ------ | ------ |
| `I` *extends* [`AnySchema`](../type-aliases/AnySchema.md) \| `undefined` | - |
| `O` *extends* [`AnySchema`](../type-aliases/AnySchema.md) \| `undefined` | - |
| `R` *extends* [`ResourceDeclaration`](ResourceDeclaration.md)[] | [`ResourceDeclaration`](ResourceDeclaration.md)[] |
| `A` *extends* [`AuthDeclaration`](AuthDeclaration.md) \| `undefined` | [`AuthDeclaration`](AuthDeclaration.md) \| `undefined` |

## Properties

| Property | Type | Description | Inherited from |
| ------ | ------ | ------ | ------ |
| <a id="summary"></a> `summary?` | `string` | - | - |
| <a id="description"></a> `description?` | `string` | - | - |
| <a id="incoming"></a> `incoming?` | `I` | Schema for messages from the client. An invalid message is answered with a `validation_failed` error envelope and dropped; the connection stays open. | - |
| <a id="outgoing"></a> `outgoing?` | `O` | Schema for messages to the client. | - |
| <a id="operationid"></a> `operationId?` | `string` | The OpenAPI `operationId`; the message schemas become `components.schemas.<OperationId>Incoming` / `…Outgoing`. | - |
| <a id="auth"></a> `auth?` | `A` | - | - |
| <a id="resources"></a> `resources?` | `R` | - | - |
| <a id="timeout"></a> `timeout?` | `string` \| `number` | Per-invocation deadline (`"5s"`, `"500ms"`, or milliseconds). The world is cancelled when it passes: an HTTP caller gets 504, an `invoke` rejects with `deadline_exceeded`, a queue message counts as a failed attempt, and a **stream** ends — the client keeps the 200 it already has and the body stops there. Undeclared: the runtime default (30 s) for requests, tasks, cron ticks, queue messages, commands, migrations and seeders; **none** for a stream, a socket or a service, which would be useless with one. A deadline you declare is honoured whatever the kind — it is the only way to bound an export. | [`WorkloadPolicies`](WorkloadPolicies.md).[`timeout`](WorkloadPolicies.md#timeout) |
| <a id="concurrency"></a> `concurrency?` | `number` | How many worlds of this workload may run at once. Past the bound the next one is refused, never queued: an HTTP request gets 503 `capacity_exhausted`, `ctx.tasks.invoke`/`dispatch` reject with the same code in the caller's world, a queue consumer simply claims fewer messages (ADR-0012). | [`WorkloadPolicies`](WorkloadPolicies.md).[`concurrency`](WorkloadPolicies.md#concurrency) |
| <a id="maxbodybytes"></a> `maxBodyBytes?` | `number` | Request body bound for this route, in bytes. A **cap**, never a raise: the effective bound is the smaller of this and the process's `USAI_MAX_BODY_BYTES` (1 MiB by default), so the operator keeps the ceiling and each route decides how much of it to accept. Declare a small one on ordinary routes and raise the process bound for the one that takes uploads, instead of opening every route to the largest body any of them needs. Above the bound the request is `413 payload_too_large`, decided before a world exists. | [`WorkloadPolicies`](WorkloadPolicies.md).[`maxBodyBytes`](WorkloadPolicies.md#maxbodybytes) |
