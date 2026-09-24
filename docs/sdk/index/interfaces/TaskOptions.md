[@sakaladev/usai](../../README.md) / [index](../README.md) / TaskOptions

# Interface: TaskOptions\<I *extends* [`AnySchema`](../type-aliases/AnySchema.md) \| `undefined`, R *extends* [`ResourceDeclaration`](ResourceDeclaration.md)[] = [`ResourceDeclaration`](ResourceDeclaration.md)[]\>

Options for [task](../functions/task.md).

## Extends

- [`WorkloadPolicies`](WorkloadPolicies.md)

## Type Parameters

| Type Parameter | Default type |
| ------ | ------ |
| `I` *extends* [`AnySchema`](../type-aliases/AnySchema.md) \| `undefined` | - |
| `R` *extends* [`ResourceDeclaration`](ResourceDeclaration.md)[] | [`ResourceDeclaration`](ResourceDeclaration.md)[] |

## Properties

| Property | Type | Description | Inherited from |
| ------ | ------ | ------ | ------ |
| <a id="timeout"></a> `timeout?` | `string` \| `number` | Per-invocation deadline (`"5s"`, `"500ms"`, or milliseconds). The world is cancelled when it passes: an HTTP caller gets 504, an `invoke` rejects with `deadline_exceeded`, a queue message counts as a failed attempt, and a **stream** ends — the client keeps the 200 it already has and the body stops there. Undeclared: the runtime default (30 s) for requests, tasks, cron ticks, queue messages, commands, migrations and seeders; **none** for a stream, a socket or a service, which would be useless with one. A deadline you declare is honoured whatever the kind — it is the only way to bound an export. | [`WorkloadPolicies`](WorkloadPolicies.md).[`timeout`](WorkloadPolicies.md#timeout) |
| <a id="concurrency"></a> `concurrency?` | `number` | How many worlds of this workload may run at once. Past the bound the next one is refused, never queued: an HTTP request gets 503 `capacity_exhausted`, `ctx.tasks.invoke`/`dispatch` reject with the same code in the caller's world, a queue consumer simply claims fewer messages (ADR-0012). | [`WorkloadPolicies`](WorkloadPolicies.md).[`concurrency`](WorkloadPolicies.md#concurrency) |
| <a id="input"></a> `input?` | `I` | Schema for the input; validated before the task's world exists. | - |
| <a id="description"></a> `description?` | `string` | A paragraph for the reference. | - |
| <a id="errors"></a> `errors?` | [`DeclaredError`](DeclaredError.md)[] | Errors the handler throws, for the reference. | - |
| <a id="resources"></a> `resources?` | `R` | Resources this task leases; `ctx.resources` is typed from this list. | - |
