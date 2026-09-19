[@sakaladev/usai](../../README.md) / [index](../README.md) / cache

# Variable: cache

```ts
const cache: {
  local: CacheLocalDeclaration<N>;
};
```

Cache resources.

## Type Declaration

| Name | Type | Description |
| ------ | ------ | ------ |
| `local()` | (`name`: `N`, `options?`: [`CacheLocalOptions`](../interfaces/CacheLocalOptions.md)) => [`CacheLocalDeclaration`](../interfaces/CacheLocalDeclaration.md)\<`N`\> | A runtime-local cache: shared by every world in this process (across revisions too), never persisted, gone on restart, not shared between replicas. Each call is one leased operation. The in-world handle is [CacheLocalHandle](../interfaces/CacheLocalHandle.md). |

## Example

```ts
const hits = cache.local("hits", { maxEntries: 10_000 });
export const count = http.post("/hits", { resources: [hits] }, async (ctx) => ({
  total: await ctx.resources.hits.increment("total"),
}));
```
