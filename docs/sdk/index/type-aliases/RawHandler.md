[@sakaladev/usai](../../README.md) / [index](../README.md) / RawHandler

# Type Alias: RawHandler

```ts
type RawHandler = (ctx: RawContext) => 
  | RawResponse
  | HttpResponse
  | Promise<
  | RawResponse
| HttpResponse>;
```

The handler of `http.raw`.

## Parameters

| Parameter | Type |
| ------ | ------ |
| `ctx` | [`RawContext`](../interfaces/RawContext.md) |

## Returns

  \| [`RawResponse`](../interfaces/RawResponse.md)
  \| [`HttpResponse`](../interfaces/HttpResponse.md)
  \| `Promise`\<
  \| [`RawResponse`](../interfaces/RawResponse.md)
  \| [`HttpResponse`](../interfaces/HttpResponse.md)\>
