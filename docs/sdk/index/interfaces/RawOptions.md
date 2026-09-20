[@sakaladev/usai](../../README.md) / [index](../README.md) / RawOptions

# Interface: RawOptions\<R *extends* [`ResourceDeclaration`](ResourceDeclaration.md)[] = [`ResourceDeclaration`](ResourceDeclaration.md)[]\>

Options for `http.raw`.

## Extends

- [`WorkloadPolicies`](WorkloadPolicies.md)

## Type Parameters

| Type Parameter | Default type |
| ------ | ------ |
| `R` *extends* [`ResourceDeclaration`](ResourceDeclaration.md)[] | [`ResourceDeclaration`](ResourceDeclaration.md)[] |

## Properties

| Property | Type | Description | Inherited from |
| ------ | ------ | ------ | ------ |
| <a id="timeout"></a> `timeout?` | `string` \| `number` | Per-invocation deadline (`"5s"`, `"500ms"`, or milliseconds). The world is cancelled when it passes: an HTTP caller gets 504, an `invoke` rejects with `deadline_exceeded`, a queue message counts as a failed attempt. Undeclared: the runtime default (30 s) for requests, none for the other kinds. | [`WorkloadPolicies`](WorkloadPolicies.md).[`timeout`](WorkloadPolicies.md#timeout) |
| <a id="concurrency"></a> `concurrency?` | `number` | How many worlds of this workload may run at once. Past the bound the next one is refused, never queued: an HTTP request gets 503 `capacity_exhausted`, `ctx.tasks.invoke`/`dispatch` reject with the same code in the caller's world, a queue consumer simply claims fewer messages (ADR-0012). | [`WorkloadPolicies`](WorkloadPolicies.md).[`concurrency`](WorkloadPolicies.md#concurrency) |
| <a id="method"></a> `method?` | [`Method`](../type-aliases/Method.md) | HTTP method. Default `POST`. | - |
| <a id="summary"></a> `summary?` | `string` | - | - |
| <a id="description"></a> `description?` | `string` | - | - |
| <a id="auth"></a> `auth?` | [`AuthDeclaration`](AuthDeclaration.md)\<`unknown`\> | - | - |
| <a id="resources"></a> `resources?` | `R` | - | - |
| <a id="errors"></a> `errors?` | [`DeclaredError`](DeclaredError.md)[] | Errors the handler answers with (documented in the reference). | - |
| <a id="responses"></a> `responses?` | `Record`\<`number`, `string`\> | Statuses the handler writes, with a description each — the reference lists them instead of "opaque response". | - |
| <a id="responseheaders"></a> `responseHeaders?` | [`ResponseHeaderDocs`](../type-aliases/ResponseHeaderDocs.md) | Response headers the handler sets, documented per status (`content-disposition`, `etag`). | - |
| <a id="operationid"></a> `operationId?` | `string` | The OpenAPI `operationId` (a generated client's method name). | - |
