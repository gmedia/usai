[@sakaladev/usai](../../README.md) / [index](../README.md) / ConsumeOptions

# Interface: ConsumeOptions\<M *extends* [`AnySchema`](../type-aliases/AnySchema.md) \| `undefined`\>

Options for `queue.consume`.

## Extends

- [`WorkloadPolicies`](WorkloadPolicies.md)

## Type Parameters

| Type Parameter |
| ------ |
| `M` *extends* [`AnySchema`](../type-aliases/AnySchema.md) \| `undefined` |

## Properties

| Property | Type | Description | Overrides | Inherited from |
| ------ | ------ | ------ | ------ | ------ |
| <a id="timeout"></a> `timeout?` | `string` \| `number` | Per-invocation deadline (`"5s"`, `"500ms"`, or milliseconds). The world is cancelled when it passes; HTTP callers get 504. Undeclared: the runtime default (30 s) for requests, none for the other kinds. | - | [`WorkloadPolicies`](WorkloadPolicies.md).[`timeout`](WorkloadPolicies.md#timeout) |
| <a id="message"></a> `message?` | `M` | Schema for the message; validated before the message's world exists. A message that fails validation is dead-lettered, not retried. | - | - |
| <a id="concurrency"></a> `concurrency?` | `number` | Messages processed at once by this consumer. Default 1. | [`WorkloadPolicies`](WorkloadPolicies.md).[`concurrency`](WorkloadPolicies.md#concurrency) | - |
| <a id="retry"></a> `retry?` | [`RetryOptions`](RetryOptions.md) | Delivery is at-least-once; declare retry to accept re-delivery. | - | - |
| <a id="database"></a> `database?` | [`PostgresDeclaration`](PostgresDeclaration.md) | PostgreSQL resource backing the queue. Default: the first declared. | - | - |
| <a id="errors"></a> `errors?` | [`DeclaredError`](DeclaredError.md)[] | - | - | - |
| <a id="resources"></a> `resources?` | [`ResourceDeclaration`](ResourceDeclaration.md)[] | - | - | - |
