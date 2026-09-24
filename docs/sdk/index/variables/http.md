[@sakaladev/usai](../../README.md) / [index](../README.md) / http

# Variable: http

```ts
const http: {
  stream: <O>(path: string, options: NoExtraKeys<O, StreamOptions<ResourceDeclaration<string, unknown>[]>>, handler: (ctx: StreamContext<O>, stream: StreamHandle) => unknown) => Workload;
  get: Declare;
  post: Declare;
  put: Declare;
  patch: Declare;
  delete: Declare;
  head: Declare;
  options: Declare;
  raw: {
   <R, A>  (path: string, options: RawOptions<R, A>, handler: RawHandler<R, A>): Workload;
     (path: string, handler: RawHandler): Workload;
  };
  response: HttpResponse<T>;
  created: HttpResponse<T>;
  accepted: HttpResponse<T>;
  noContent: HttpResponse<null>;
  notModified: HttpResponse<null>;
  rawResponse: RawResponse;
};
```

Declare HTTP endpoints. Each `http.<method>(path, options, handler)`
returns a [Workload](../interfaces/Workload.md) to list in `defineApp`/`defineModule`.

A request is a **finite** unit of work: the runtime routes it, decodes
it and validates `params`/`query`/`headers`/`body` against the declared
schemas **before a world exists**, so a 400 never runs application
code. Then a fresh world runs the `auth` resolver (401 stops there) and
the handler with an [HttpContext](../interfaces/HttpContext.md), bounded by `timeout` (the
runtime default when undeclared) and `concurrency`. When the handler returns,
the world ends: a pending `ctx.tasks.invoke`, an open transaction or an
un-awaited resource operation at that point is a lifecycle error, not a
silent drop (hand work off with `ctx.tasks.dispatch` instead).

`response` is one schema (status 200) or a map of status to schema; a
plain return value takes the lowest declared 2xx status (200 without a
declaration; 204 when it returns nothing), `http.response`/`created`/…
pick another. `errors` lists the `{ code, status }` pairs the handler
throws, `summary`/`description` describe the operation, so the
reference page and the OpenAPI document can say so.

Paths: `/users/:id` declares a parameter. A literal segment always wins
over a parameter at the same position (`/invoices/summary` beats
`/invoices/:id`, whatever the declaration order); the same method and
path twice is a build error; a path nobody declares is 404
`route_not_found` before any world exists.

## Type Declaration

| Name | Type | Default value | Description |
| ------ | ------ | ------ | ------ |
| <a id="property-stream"></a> `stream()` | \<`O`\>(`path`: `string`, `options`: `NoExtraKeys`\<`O`, [`StreamOptions`](../interfaces/StreamOptions.md)\<[`ResourceDeclaration`](../interfaces/ResourceDeclaration.md)\<`string`, `unknown`\>[]\>\>, `handler`: (`ctx`: [`StreamContext`](../interfaces/StreamContext.md)\<`O`\>, `stream`: [`StreamHandle`](../interfaces/StreamHandle.md)) => `unknown`) => [`Workload`](../interfaces/Workload.md) | `streams.stream` | Declare a streaming endpoint (`http.stream`): a **connection-bound** world that lives until the handler returns. `params` and `query` are validated before the world exists; `timeout` bounds the whole stream. Chunks are `text/event-stream` by default (`stream.event(name, data, { id })` writes one server-sent event; a reconnecting `EventSource` sends the last `id` back as `ctx.headers["last-event-id"]`); set `content-type` in `start` for anything else. A client that leaves cancels the world — the normal end of a stream, not a failure. **Example** `export const events = http.stream("/events", { resources: [cache] }, async (ctx, stream) => { while (!ctx.signal.aborted) { await stream.event("tick", { total: await ctx.resources.cache.get("total") }); await ctx.sleep("1s"); } });` |
| <a id="property-get"></a> `get` | [`Declare`](../type-aliases/Declare.md) | - | `GET` endpoint. |
| <a id="property-post"></a> `post` | [`Declare`](../type-aliases/Declare.md) | - | `POST` endpoint. |
| <a id="property-put"></a> `put` | [`Declare`](../type-aliases/Declare.md) | - | `PUT` endpoint. |
| <a id="property-patch"></a> `patch` | [`Declare`](../type-aliases/Declare.md) | - | `PATCH` endpoint. |
| <a id="property-delete"></a> `delete` | [`Declare`](../type-aliases/Declare.md) | - | `DELETE` endpoint. |
| <a id="property-head"></a> `head` | [`Declare`](../type-aliases/Declare.md) | - | `HEAD` endpoint. |
| <a id="property-options"></a> `options` | [`Declare`](../type-aliases/Declare.md) | - | `OPTIONS` endpoint. |
| <a id="property-raw"></a> `raw()` | \{ \<`R`, `A`\> (`path`: `string`, `options`: [`RawOptions`](../interfaces/RawOptions.md)\<`R`, `A`\>, `handler`: [`RawHandler`](../type-aliases/RawHandler.md)\<`R`, `A`\>): [`Workload`](../interfaces/Workload.md); (`path`: `string`, `handler`: [`RawHandler`](../type-aliases/RawHandler.md)): [`Workload`](../interfaces/Workload.md); \} | - | Low-level escape hatch: exact bytes in ([RawContext](../interfaces/RawContext.md)), a [RawResponse](../interfaces/RawResponse.md) or [HttpResponse](../interfaces/HttpResponse.md) out. No schema validation; the reference shows the statuses from `responses`. |
| `response()` | ( `status`: `number`, `body`: `T`, `headers?`: `ResponseHeaders` ) => [`HttpResponse`](../interfaces/HttpResponse.md)\<`T`\> | - | An explicit status and headers around a contract-encoded body. |
| `created()` | (`body`: `T`, `headers?`: `ResponseHeaders`) => [`HttpResponse`](../interfaces/HttpResponse.md)\<`T`\> | - | `201 Created` with a body. |
| `accepted()` | (`body`: `T`, `headers?`: `ResponseHeaders`) => [`HttpResponse`](../interfaces/HttpResponse.md)\<`T`\> | - | `202 Accepted` with a body: the work continues elsewhere (a dispatched task). |
| `noContent()` | (`headers?`: `ResponseHeaders`) => [`HttpResponse`](../interfaces/HttpResponse.md)\<`null`\> | - | `204 No Content`. |
| `notModified()` | (`headers?`: `ResponseHeaders`) => [`HttpResponse`](../interfaces/HttpResponse.md)\<`null`\> | - | `304 Not Modified`, for a conditional GET whose `if-none-match` matched the `etag` you computed: no body, the cache headers repeated so the client keeps its copy (`http.notModified({ etag, "cache-control": "private, max-age=0" })`). Needs no entry in the `response` contract. |
| `rawResponse()` | ( `status`: `number`, `body`: `string` \| `Uint8Array`\<`ArrayBufferLike`\>, `headers?`: `ResponseHeaders` ) => [`RawResponse`](../interfaces/RawResponse.md) | - | Raw text or bytes with an explicit status, for `http.raw` handlers. |

## Example

```ts
export const createUser = http.post(
  "/users",
  { body: NewUser, response: { 201: User }, auth: session, resources: [db], errors: [{ code: "conflict", status: 409 }] },
  async (ctx) => {
    const row = await ctx.resources.db.one<User>("insert into users … returning *", [ctx.body.name]);
    await ctx.tasks.dispatch(sendWelcome, { userId: row.id }); // outlives the response, explicitly
    return http.created(row);
  },
);
```
