[@sakaladev/usai](../../README.md) / [index](../README.md) / RawOptions

# Interface: RawOptions

Options for `http.raw`.

## Extends

- [`WorkloadPolicies`](WorkloadPolicies.md)

## Properties

| Property | Type | Description | Inherited from |
| ------ | ------ | ------ | ------ |
| <a id="timeout"></a> `timeout?` | `string` \| `number` | Per-invocation deadline (`"5s"`, `"500ms"`, or milliseconds). The world is cancelled when it passes; HTTP callers get 504. Undeclared: the runtime default (30 s) for requests, none for the other kinds. | [`WorkloadPolicies`](WorkloadPolicies.md).[`timeout`](WorkloadPolicies.md#timeout) |
| <a id="concurrency"></a> `concurrency?` | `number` | How many worlds of this workload may run at once; the next request is refused with 503 `capacity_exhausted`, not queued (ADR-0012). | [`WorkloadPolicies`](WorkloadPolicies.md).[`concurrency`](WorkloadPolicies.md#concurrency) |
| <a id="method"></a> `method?` | [`Method`](../type-aliases/Method.md) | HTTP method. Default `POST`. | - |
| <a id="auth"></a> `auth?` | [`AuthDeclaration`](AuthDeclaration.md)\<`unknown`\> | - | - |
| <a id="resources"></a> `resources?` | [`ResourceDeclaration`](ResourceDeclaration.md)[] | - | - |
| <a id="errors"></a> `errors?` | [`DeclaredError`](DeclaredError.md)[] | Errors the handler answers with (documented in the reference). | - |
| <a id="responses"></a> `responses?` | `Record`\<`number`, `string`\> | Statuses the handler writes, with a description each — the reference lists them instead of "opaque response". | - |
