[@sakaladev/usai](../../README.md) / [index](../README.md) / RawHandler

# Type Alias: RawHandler\<R *extends* [`ResourceDeclaration`](../interfaces/ResourceDeclaration.md)[] = [`ResourceDeclaration`](../interfaces/ResourceDeclaration.md)[], A *extends* [`AuthDeclaration`](../interfaces/AuthDeclaration.md) \| `undefined` = `undefined`\>

```ts
type RawHandler<R extends ResourceDeclaration[] = ResourceDeclaration[], A extends AuthDeclaration | undefined = undefined> = (ctx: RawContext<R, A>) => 
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
| `A` *extends* [`AuthDeclaration`](../interfaces/AuthDeclaration.md) \| `undefined` | `undefined` |

## Parameters

| Parameter | Type |
| ------ | ------ |
| `ctx` | [`RawContext`](../interfaces/RawContext.md)\<`R`, `A`\> |

## Returns

  \| [`RawResponse`](../interfaces/RawResponse.md)
  \| [`HttpResponse`](../interfaces/HttpResponse.md)
  \| `Promise`\<
  \| [`RawResponse`](../interfaces/RawResponse.md)
  \| [`HttpResponse`](../interfaces/HttpResponse.md)\>
