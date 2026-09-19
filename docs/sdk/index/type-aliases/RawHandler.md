[@sakaladev/usai](../../README.md) / [index](../README.md) / RawHandler

# Type Alias: RawHandler\<R *extends* [`ResourceDeclaration`](../interfaces/ResourceDeclaration.md)[] = [`ResourceDeclaration`](../interfaces/ResourceDeclaration.md)[]\>

```ts
type RawHandler<R extends ResourceDeclaration[] = ResourceDeclaration[]> = (ctx: RawContext<R>) => 
  | RawResponse
  | HttpResponse
  | Promise<
  | RawResponse
| HttpResponse>;
```

The handler of `http.raw`.

## Type Parameters

| Type Parameter | Default type |
| ------ | ------ |
| `R` *extends* [`ResourceDeclaration`](../interfaces/ResourceDeclaration.md)[] | [`ResourceDeclaration`](../interfaces/ResourceDeclaration.md)[] |

## Parameters

| Parameter | Type |
| ------ | ------ |
| `ctx` | [`RawContext`](../interfaces/RawContext.md)\<`R`\> |

## Returns

  \| [`RawResponse`](../interfaces/RawResponse.md)
  \| [`HttpResponse`](../interfaces/HttpResponse.md)
  \| `Promise`\<
  \| [`RawResponse`](../interfaces/RawResponse.md)
  \| [`HttpResponse`](../interfaces/HttpResponse.md)\>
