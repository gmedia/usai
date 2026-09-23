[@sakaladev/usai](../../README.md) / [index](../README.md) / Declare

# Type Alias: Declare

```ts
type Declare = <O>(path: string, options: NoExtraKeys<O, HttpOptions>, handler: (ctx: HttpContext<O>) => HttpHandlerResult<ResponseOf<O>>) => Workload;
```

The signature of `http.get`/`post`/….

## Type Parameters

| Type Parameter |
| ------ |
| `O` *extends* [`HttpOptions`](../interfaces/HttpOptions.md) |

## Parameters

| Parameter | Type |
| ------ | ------ |
| `path` | `string` |
| `options` | `NoExtraKeys`\<`O`, [`HttpOptions`](../interfaces/HttpOptions.md)\> |
| `handler` | (`ctx`: [`HttpContext`](../interfaces/HttpContext.md)\<`O`\>) => [`HttpHandlerResult`](HttpHandlerResult.md)\<`ResponseOf`\<`O`\>\> |

## Returns

[`Workload`](../interfaces/Workload.md)
