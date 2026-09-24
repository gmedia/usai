[@sakaladev/usai](../../README.md) / [index](../README.md) / HttpOptions

# Interface: HttpOptions

The schema slots of an HTTP endpoint. Any Standard Schema
(`zod`, `valibot`, `arktype`, …) works; slots whose schema can describe
itself as JSON Schema are validated before a world exists, the others
inside it.

## Extends

- [`HttpContracts`](HttpContracts.md).[`WorkloadPolicies`](WorkloadPolicies.md)

## Properties

| Property | Type | Description | Inherited from |
| ------ | ------ | ------ | ------ |
| <a id="timeout"></a> `timeout?` | `string` \| `number` | Per-invocation deadline (`"5s"`, `"500ms"`, or milliseconds). For **finite** work the world is cancelled when it passes: an HTTP caller gets 504, an `invoke` rejects with `deadline_exceeded`, a queue message counts as a failed attempt. Undeclared, finite work gets the runtime default of 30 s. For **connection-bound and persistent** work — a stream, a socket, a service — there is **no default**, because one that ended after 30 s would be useless. A deadline you declare is honoured, and it **stops** the world rather than cancelling it: `ctx.signal` aborts, a pending `ctx.sleep` returns, the handler can finish what it is doing, and a socket's connection is closed so its `close` handler runs. It has one second to unwind and is then cancelled, so a handler that never checks `ctx.signal` still stops near the bound you declared. For a stream the client keeps the `200` it already has and the body stops there — with no trailer and no error, so end an export with a sentinel the reader requires. | [`WorkloadPolicies`](WorkloadPolicies.md).[`timeout`](WorkloadPolicies.md#timeout) |
| <a id="concurrency"></a> `concurrency?` | `number` | How many worlds of this workload may run at once. Past the bound the next one is refused, never queued: an HTTP request gets 503 `capacity_exhausted`, `ctx.tasks.invoke`/`dispatch` reject with the same code in the caller's world, a queue consumer simply claims fewer messages (ADR-0012). | [`WorkloadPolicies`](WorkloadPolicies.md).[`concurrency`](WorkloadPolicies.md#concurrency) |
| <a id="maxbodybytes"></a> `maxBodyBytes?` | `number` | Request body bound for this route, in bytes. A **cap**, never a raise: the effective bound is the smaller of this and the process's `USAI_MAX_BODY_BYTES` (1 MiB by default), so the operator keeps the ceiling and each route decides how much of it to accept. Declare a small one on ordinary routes and raise the process bound for the one that takes uploads, instead of opening every route to the largest body any of them needs. Above the bound the request is `413 payload_too_large`, decided before a world exists. | [`WorkloadPolicies`](WorkloadPolicies.md).[`maxBodyBytes`](WorkloadPolicies.md#maxbodybytes) |
| <a id="params"></a> `params?` | [`AnySchema`](../type-aliases/AnySchema.md) | Path parameters (`/users/:id` → `{ id }`); strings before coercion. | [`HttpContracts`](HttpContracts.md).[`params`](HttpContracts.md#params) |
| <a id="query"></a> `query?` | [`AnySchema`](../type-aliases/AnySchema.md) | Query string; a repeated key arrives as an array. | [`HttpContracts`](HttpContracts.md).[`query`](HttpContracts.md#query) |
| <a id="headers"></a> `headers?` | [`AnySchema`](../type-aliases/AnySchema.md) | Request headers, lower-cased names. | [`HttpContracts`](HttpContracts.md).[`headers`](HttpContracts.md#headers) |
| <a id="body"></a> `body?` | [`AnySchema`](../type-aliases/AnySchema.md) | JSON request body. | [`HttpContracts`](HttpContracts.md).[`body`](HttpContracts.md#body) |
| <a id="response"></a> `response?` | \| [`AnySchema`](../type-aliases/AnySchema.md) \| `Record`\<`number`, [`AnySchema`](../type-aliases/AnySchema.md)\> | One schema (status 200) or a map of status to schema. A plain return value is encoded with the lowest declared 2xx status and checked against its schema (`response_contract_violation`, 500, otherwise); `http.response(status, body)` picks another declared status. | [`HttpContracts`](HttpContracts.md).[`response`](HttpContracts.md#response) |
| <a id="summary"></a> `summary?` | `string` | One line for the reference and the OpenAPI `summary`. Without it the operation is shown by method and path. | - |
| <a id="description"></a> `description?` | `string` | A paragraph for the reference and the OpenAPI `description`. | - |
| <a id="errors"></a> `errors?` | [`DeclaredError`](DeclaredError.md)[] | Errors the handler throws, for the reference and the OpenAPI document. | - |
| <a id="responseheaders"></a> `responseHeaders?` | `Partial`\<`Record`\<`number` \| `"*"`, `Record`\<`string`, `string`\>\>\> | Response headers the handler sets (`set-cookie`, `location`, `etag`), documented per status. See [ResponseHeaderDocs](../type-aliases/ResponseHeaderDocs.md). | - |
| <a id="operationid"></a> `operationId?` | `string` | The OpenAPI `operationId` — what a generated client names the method (`listProducts`). Derived from method and path when absent (`getProducts`). Unique per application. | - |
| <a id="auth"></a> `auth?` | [`AuthDeclaration`](AuthDeclaration.md)\<`unknown`, readonly [`ResourceDeclaration`](ResourceDeclaration.md)\<`string`, `unknown`\>[]\> | The authentication boundary; its principal is `ctx.auth`. | - |
| <a id="resources"></a> `resources?` | [`ResourceDeclaration`](ResourceDeclaration.md)\<`string`, `unknown`\>[] | Resources this endpoint leases; only these are on `ctx.resources`. | - |
