[@sakaladev/usai](../../README.md) / [index](../README.md) / ResourcesOf

# Type Alias: ResourcesOf\<R\>

```ts
type ResourcesOf<R> = R extends readonly ResourceDeclaration[] ? { readonly [D in R[number] as D["name"]]: D extends ResourceDeclaration<string, infer H> ? H : unknown } : Record<string, unknown>;
```

`ctx.resources` for a workload that declared `resources: R`: one
property per declaration, named by the resource, typed as its in-world
handle ([PostgresHandle](../interfaces/PostgresHandle.md), [CacheLocalHandle](../interfaces/CacheLocalHandle.md),
[HttpClientHandle](../interfaces/HttpClientHandle.md)). Without a declaration list it is
`Record<string, unknown>`.

## Type Parameters

| Type Parameter |
| ------ |
| `R` |
