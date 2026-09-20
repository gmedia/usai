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
| <a id="auth"></a> `auth?` | [`AuthDeclaration`](AuthDeclaration.md)\<`unknown`\> | - | - |
| <a id="resources"></a> `resources?` | `R` | - | - |
| <a id="params"></a> `params?` | [`AnySchema`](../type-aliases/AnySchema.md) | - | - |
| <a id="query"></a> `query?` | [`AnySchema`](../type-aliases/AnySchema.md) | - | - |
| <a id="contenttype"></a> `contentType?` | `string` | The stream's media type: `text/event-stream` (default; an SSE endpoint), `text/csv`, `application/x-ndjson`, … Sets the response `content-type` unless `stream.start({ headers })` says otherwise, and is what the OpenAPI document and the reference say the endpoint streams. | - |
| <a id="responseheaders"></a> `responseHeaders?` | [`ResponseHeaderDocs`](../type-aliases/ResponseHeaderDocs.md) | Response headers the stream sets (`content-disposition` for a download), documented. | - |
| <a id="operationid"></a> `operationId?` | `string` | The OpenAPI `operationId` (a generated client's method name). | - |
| <a id="events"></a> `events?` | `Record`\<`string`, [`AnySchema`](../type-aliases/AnySchema.md)\> | The server-sent events this stream emits, by name, with the schema of each `data:` payload: `events: { tick: z.object({ i: z.number() }) }`. `stream.event("tick", data)` validates `data` against it (a mismatch is a `500 event_contract_violation` — the stream ends early and the log says which event), and the OpenAPI document names them as `components.schemas.<OperationId>EventTick` with the event list in the response description, so a client knows what to `addEventListener` for. | - |
| <a id="timeout"></a> `timeout?` | `string` \| `number` | Per-invocation deadline (`"5s"`, `"500ms"`, or milliseconds). The world is cancelled when it passes: an HTTP caller gets 504, an `invoke` rejects with `deadline_exceeded`, a queue message counts as a failed attempt. Undeclared: the runtime default (30 s) for requests, none for the other kinds. | [`WorkloadPolicies`](WorkloadPolicies.md).[`timeout`](WorkloadPolicies.md#timeout) |
| <a id="concurrency"></a> `concurrency?` | `number` | How many worlds of this workload may run at once. Past the bound the next one is refused, never queued: an HTTP request gets 503 `capacity_exhausted`, `ctx.tasks.invoke`/`dispatch` reject with the same code in the caller's world, a queue consumer simply claims fewer messages (ADR-0012). | [`WorkloadPolicies`](WorkloadPolicies.md).[`concurrency`](WorkloadPolicies.md#concurrency) |
