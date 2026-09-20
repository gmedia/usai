[@sakaladev/usai](../../README.md) / [index](../README.md) / ResolverContext

# Type Alias: ResolverContext\<R\>

```ts
type ResolverContext<R> = Omit<BaseContext, "resources"> & {
  resources: ResourcesOf<R>;
  request: AuthRequest;
};
```

What a resolver runs with: the workload's context plus the request's
envelope; `resources` typed from the scheme's own `resources: [...]`.

## Type Declaration

| Name | Type |
| ------ | ------ |
| `resources` | [`ResourcesOf`](ResourcesOf.md)\<`R`\> |
| `request` | [`AuthRequest`](../interfaces/AuthRequest.md) |

## Type Parameters

| Type Parameter |
| ------ |
| `R` |
