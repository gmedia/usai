[@sakaladev/usai](../../README.md) / [index](../README.md) / HttpHandlerResult

# Type Alias: HttpHandlerResult\<T\>

```ts
type HttpHandlerResult<T> = 
  | T
  | HttpResponse<T>
  | HttpResponse<null>
  | RawResponse
  | Promise<
  | T
  | HttpResponse<T>
  | HttpResponse<null>
| RawResponse>;
```

What an HTTP handler may return: the body (encoded as JSON with status
200, or the single declared `response` status), an explicit
[HttpResponse](../interfaces/HttpResponse.md) from `http.response`/`http.created`/…, a bodiless
one (`http.noContent()`, `http.notModified({ etag })`) whatever the
declared contract, or a [RawResponse](../interfaces/RawResponse.md). Promises of any of these are
awaited.

## Type Parameters

| Type Parameter |
| ------ |
| `T` |
