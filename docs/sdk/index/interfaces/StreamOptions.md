[@sakaladev/usai](../../README.md) / [index](../README.md) / StreamOptions

# Interface: StreamOptions

Options for `http.stream`.

## Extends

- [`WorkloadPolicies`](WorkloadPolicies.md)

## Properties

| Property | Type | Description | Inherited from |
| ------ | ------ | ------ | ------ |
| <a id="method"></a> `method?` | [`Method`](../type-aliases/Method.md) | Default `GET`. | - |
| <a id="auth"></a> `auth?` | [`AuthDeclaration`](AuthDeclaration.md)\<`unknown`\> | - | - |
| <a id="resources"></a> `resources?` | [`ResourceDeclaration`](ResourceDeclaration.md)[] | - | - |
| <a id="params"></a> `params?` | [`AnySchema`](../type-aliases/AnySchema.md) | - | - |
| <a id="query"></a> `query?` | [`AnySchema`](../type-aliases/AnySchema.md) | - | - |
| <a id="timeout"></a> `timeout?` | `string` \| `number` | Per-invocation deadline (`"5s"`, `"500ms"`, or milliseconds). The world is cancelled when it passes; HTTP callers get 504. Undeclared: the runtime default (30 s) for requests, none for the other kinds. | [`WorkloadPolicies`](WorkloadPolicies.md).[`timeout`](WorkloadPolicies.md#timeout) |
| <a id="concurrency"></a> `concurrency?` | `number` | How many worlds of this workload may run at once; the next request is refused with 503 `capacity_exhausted`, not queued (ADR-0012). | [`WorkloadPolicies`](WorkloadPolicies.md).[`concurrency`](WorkloadPolicies.md#concurrency) |
