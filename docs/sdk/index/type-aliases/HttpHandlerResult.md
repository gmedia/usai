[@sakaladev/usai](../../README.md) / [index](../README.md) / HttpHandlerResult

# Type Alias: HttpHandlerResult\<T\>

```ts
type HttpHandlerResult<T> = 
  | T
  | HttpResponse<T>
  | RawResponse
  | Promise<
  | T
  | HttpResponse<T>
| RawResponse>;
```

What an HTTP handler may return: the body (encoded as JSON with status
200, or the single declared `response` status), an explicit
[HttpResponse](../interfaces/HttpResponse.md) from `http.response`/`http.created`/…, or a
[RawResponse](../interfaces/RawResponse.md). Promises of any of these are awaited.

## Type Parameters

| Type Parameter |
| ------ |
| `T` |
