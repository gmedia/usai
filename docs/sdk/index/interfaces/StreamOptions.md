[@sakaladev/usai](../../README.md) / [index](../README.md) / StreamOptions

# Interface: StreamOptions\<R *extends* [`ResourceDeclaration`](ResourceDeclaration.md)[] = [`ResourceDeclaration`](ResourceDeclaration.md)[]\>

Options for `http.stream`.

## Extends

- [`WorkloadPolicies`](WorkloadPolicies.md)

## Type Parameters

| Type Parameter | Default type |
| ------ | ------ |
| `R` *extends* [`ResourceDeclaration`](ResourceDeclaration.md)[] | [`ResourceDeclaration`](ResourceDeclaration.md)[] |

## Properties

| Property | Type | Description | Inherited from |
| ------ | ------ | ------ | ------ |
| <a id="method"></a> `method?` | [`Method`](../type-aliases/Method.md) | Default `GET`. | - |
| <a id="summary"></a> `summary?` | `string` | - | - |
| <a id="description"></a> `description?` | `string` | - | - |
| <a id="auth"></a> `auth?` | [`AuthDeclaration`](AuthDeclaration.md)\<`unknown`, readonly [`ResourceDeclaration`](ResourceDeclaration.md)\<`string`, `unknown`\>[]\> | - | - |
| <a id="resources"></a> `resources?` | `R` | - | - |
| <a id="params"></a> `params?` | [`AnySchema`](../type-aliases/AnySchema.md) | - | - |
| <a id="query"></a> `query?` | [`AnySchema`](../type-aliases/AnySchema.md) | - | - |
| <a id="contenttype"></a> `contentType?` | `string` | The stream's media type: `text/event-stream` (default; an SSE endpoint), `text/csv`, `application/x-ndjson`, … Sets the response `content-type` unless `stream.start({ headers })` says otherwise, and is what the OpenAPI document and the reference say the endpoint streams. | - |
| <a id="responseheaders"></a> `responseHeaders?` | [`ResponseHeaderDocs`](../type-aliases/ResponseHeaderDocs.md) | Response headers the stream sets (`content-disposition` for a download), documented. | - |
| <a id="operationid"></a> `operationId?` | `string` | The OpenAPI `operationId` (a generated client's method name). | - |
| <a id="events"></a> `events?` | `Record`\<`string`, [`AnySchema`](../type-aliases/AnySchema.md)\> | The server-sent events this stream emits, by name, with the schema of each `data:` payload: `events: { tick: z.object({ i: z.number() }) }`. `stream.event("tick", data)` validates `data` against it (a mismatch is a `500 event_contract_violation` — the stream ends early and the log says which event), and the OpenAPI document names them as `components.schemas.<OperationId>EventTick` with the event list in the response description, so a client knows what to `addEventListener` for. | - |
| <a id="timeout"></a> `timeout?` | `string` \| `number` | Per-invocation deadline (`"5s"`, `"500ms"`, or milliseconds). For **finite** work the world is cancelled when it passes: an HTTP caller gets 504, an `invoke` rejects with `deadline_exceeded`, a queue message counts as a failed attempt. Undeclared, finite work gets the runtime default of 30 s. For **connection-bound and persistent** work — a stream, a socket, a service — there is **no default**, because one that ended after 30 s would be useless. A deadline you declare is honoured, and it **stops** the world rather than cancelling it: `ctx.signal` aborts, a pending `ctx.sleep` returns, the handler can finish what it is doing, and a socket's connection is closed so its `close` handler runs. For a stream the client keeps the `200` it already has and the body stops there — with no trailer and no error, so end an export with a sentinel the reader requires. | [`WorkloadPolicies`](WorkloadPolicies.md).[`timeout`](WorkloadPolicies.md#timeout) |
| <a id="concurrency"></a> `concurrency?` | `number` | How many worlds of this workload may run at once. Past the bound the next one is refused, never queued: an HTTP request gets 503 `capacity_exhausted`, `ctx.tasks.invoke`/`dispatch` reject with the same code in the caller's world, a queue consumer simply claims fewer messages (ADR-0012). | [`WorkloadPolicies`](WorkloadPolicies.md).[`concurrency`](WorkloadPolicies.md#concurrency) |
| <a id="maxbodybytes"></a> `maxBodyBytes?` | `number` | Request body bound for this route, in bytes. A **cap**, never a raise: the effective bound is the smaller of this and the process's `USAI_MAX_BODY_BYTES` (1 MiB by default), so the operator keeps the ceiling and each route decides how much of it to accept. Declare a small one on ordinary routes and raise the process bound for the one that takes uploads, instead of opening every route to the largest body any of them needs. Above the bound the request is `413 payload_too_large`, decided before a world exists. | [`WorkloadPolicies`](WorkloadPolicies.md).[`maxBodyBytes`](WorkloadPolicies.md#maxbodybytes) |
