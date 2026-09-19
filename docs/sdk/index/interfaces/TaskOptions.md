[@sakaladev/usai](../../README.md) / [index](../README.md) / TaskOptions

# Interface: TaskOptions\<I *extends* [`AnySchema`](../type-aliases/AnySchema.md) \| `undefined`\>

Options for [task](../functions/task.md).

## Extends

- [`WorkloadPolicies`](WorkloadPolicies.md)

## Type Parameters

| Type Parameter |
| ------ |
| `I` *extends* [`AnySchema`](../type-aliases/AnySchema.md) \| `undefined` |

## Properties

| Property | Type | Description | Inherited from |
| ------ | ------ | ------ | ------ |
| <a id="timeout"></a> `timeout?` | `string` \| `number` | Per-invocation deadline (`"5s"`, `"500ms"`, or milliseconds). The world is cancelled when it passes; HTTP callers get 504. Undeclared: the runtime default (30 s) for requests, none for the other kinds. | [`WorkloadPolicies`](WorkloadPolicies.md).[`timeout`](WorkloadPolicies.md#timeout) |
| <a id="concurrency"></a> `concurrency?` | `number` | How many worlds of this workload may run at once; the next request is refused with 503 `capacity_exhausted`, not queued (ADR-0012). | [`WorkloadPolicies`](WorkloadPolicies.md).[`concurrency`](WorkloadPolicies.md#concurrency) |
| <a id="input"></a> `input?` | `I` | Schema for the input; validated before the task's world exists. | - |
| <a id="errors"></a> `errors?` | [`DeclaredError`](DeclaredError.md)[] | Errors the handler throws, for the reference. | - |
| <a id="resources"></a> `resources?` | [`ResourceDeclaration`](ResourceDeclaration.md)[] | Resources this task leases. | - |
