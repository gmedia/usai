[@sakaladev/usai](../../README.md) / [index](../README.md) / HttpOptions

# Interface: HttpOptions

Options of `http.get`/`post`/…: contracts, policies, errors, auth, resources.

## Extends

- [`HttpContracts`](HttpContracts.md).[`WorkloadPolicies`](WorkloadPolicies.md)

## Properties

| Property | Type | Description | Inherited from |
| ------ | ------ | ------ | ------ |
| <a id="timeout"></a> `timeout?` | `string` \| `number` | Per-invocation deadline (`"5s"`, `"500ms"`, or milliseconds). The world is cancelled when it passes; HTTP callers get 504. Undeclared: the runtime default (30 s) for requests, none for the other kinds. | [`WorkloadPolicies`](WorkloadPolicies.md).[`timeout`](WorkloadPolicies.md#timeout) |
| <a id="concurrency"></a> `concurrency?` | `number` | How many worlds of this workload may run at once; the next request is refused with 503 `capacity_exhausted`, not queued (ADR-0012). | [`WorkloadPolicies`](WorkloadPolicies.md).[`concurrency`](WorkloadPolicies.md#concurrency) |
| <a id="params"></a> `params?` | [`AnySchema`](../type-aliases/AnySchema.md) | - | [`HttpContracts`](HttpContracts.md).[`params`](HttpContracts.md#params) |
| <a id="query"></a> `query?` | [`AnySchema`](../type-aliases/AnySchema.md) | - | [`HttpContracts`](HttpContracts.md).[`query`](HttpContracts.md#query) |
| <a id="headers"></a> `headers?` | [`AnySchema`](../type-aliases/AnySchema.md) | - | [`HttpContracts`](HttpContracts.md).[`headers`](HttpContracts.md#headers) |
| <a id="body"></a> `body?` | [`AnySchema`](../type-aliases/AnySchema.md) | - | [`HttpContracts`](HttpContracts.md).[`body`](HttpContracts.md#body) |
| <a id="response"></a> `response?` | \| [`AnySchema`](../type-aliases/AnySchema.md) \| `Record`\<`number`, [`AnySchema`](../type-aliases/AnySchema.md)\> | - | [`HttpContracts`](HttpContracts.md).[`response`](HttpContracts.md#response) |
| <a id="errors"></a> `errors?` | [`DeclaredError`](DeclaredError.md)[] | Errors the handler throws, for the reference and the OpenAPI document. | - |
| <a id="auth"></a> `auth?` | [`AuthDeclaration`](AuthDeclaration.md)\<`unknown`\> | The authentication boundary; its principal is `ctx.auth`. | - |
| <a id="resources"></a> `resources?` | [`ResourceDeclaration`](ResourceDeclaration.md)[] | Resources this endpoint leases; only these are on `ctx.resources`. | - |
