[@sakaladev/usai](../../README.md) / [index](../README.md) / HttpOptions

# Interface: HttpOptions

Options of `http.get`/`post`/…: contracts, policies, errors, auth, resources.

## Extends

- [`HttpContracts`](HttpContracts.md).[`WorkloadPolicies`](WorkloadPolicies.md)

## Properties

| Property | Type | Description | Inherited from |
| ------ | ------ | ------ | ------ |
| <a id="timeout"></a> `timeout?` | `string` \| `number` | Per-invocation deadline (`"5s"`, `"500ms"`, or milliseconds). The world is cancelled when it passes: an HTTP caller gets 504, an `invoke` rejects with `deadline_exceeded`, a queue message counts as a failed attempt. Undeclared: the runtime default (30 s) for requests, none for the other kinds. | [`WorkloadPolicies`](WorkloadPolicies.md).[`timeout`](WorkloadPolicies.md#timeout) |
| <a id="concurrency"></a> `concurrency?` | `number` | How many worlds of this workload may run at once. Past the bound the next one is refused, never queued: an HTTP request gets 503 `capacity_exhausted`, `ctx.tasks.invoke`/`dispatch` reject with the same code in the caller's world, a queue consumer simply claims fewer messages (ADR-0012). | [`WorkloadPolicies`](WorkloadPolicies.md).[`concurrency`](WorkloadPolicies.md#concurrency) |
| <a id="params"></a> `params?` | [`AnySchema`](../type-aliases/AnySchema.md) | Path parameters (`/users/:id` → `{ id }`); strings before coercion. | [`HttpContracts`](HttpContracts.md).[`params`](HttpContracts.md#params) |
| <a id="query"></a> `query?` | [`AnySchema`](../type-aliases/AnySchema.md) | Query string; a repeated key arrives as an array. | [`HttpContracts`](HttpContracts.md).[`query`](HttpContracts.md#query) |
| <a id="headers"></a> `headers?` | [`AnySchema`](../type-aliases/AnySchema.md) | Request headers, lower-cased names. | [`HttpContracts`](HttpContracts.md).[`headers`](HttpContracts.md#headers) |
| <a id="body"></a> `body?` | [`AnySchema`](../type-aliases/AnySchema.md) | JSON request body. | [`HttpContracts`](HttpContracts.md).[`body`](HttpContracts.md#body) |
| <a id="response"></a> `response?` | \| [`AnySchema`](../type-aliases/AnySchema.md) \| `Record`\<`number`, [`AnySchema`](../type-aliases/AnySchema.md)\> | One schema (status 200) or a map of status to schema. A plain return value is encoded with the lowest declared 2xx status and checked against its schema (`response_contract_violation`, 500, otherwise); `http.response(status, body)` picks another declared status. | [`HttpContracts`](HttpContracts.md).[`response`](HttpContracts.md#response) |
| <a id="summary"></a> `summary?` | `string` | One line for the reference and the OpenAPI `summary`. Without it the operation is shown by method and path. | - |
| <a id="description"></a> `description?` | `string` | A paragraph for the reference and the OpenAPI `description`. | - |
| <a id="errors"></a> `errors?` | [`DeclaredError`](DeclaredError.md)[] | Errors the handler throws, for the reference and the OpenAPI document. | - |
| <a id="responseheaders"></a> `responseHeaders?` | [`ResponseHeaderDocs`](../type-aliases/ResponseHeaderDocs.md) | Response headers the handler sets (`set-cookie`, `location`, `etag`), documented per status. See [ResponseHeaderDocs](../type-aliases/ResponseHeaderDocs.md). | - |
| <a id="operationid"></a> `operationId?` | `string` | The OpenAPI `operationId` — what a generated client names the method (`listProducts`). Derived from method and path when absent (`getProducts`). Unique per application. | - |
| <a id="auth"></a> `auth?` | [`AuthDeclaration`](AuthDeclaration.md)\<`unknown`\> | The authentication boundary; its principal is `ctx.auth`. | - |
| <a id="resources"></a> `resources?` | [`ResourceDeclaration`](ResourceDeclaration.md)\<`string`, `unknown`\>[] | Resources this endpoint leases; only these are on `ctx.resources`. | - |
