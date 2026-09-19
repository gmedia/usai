[@sakaladev/usai](../../README.md) / [index](../README.md) / seeder

# Function: seeder()

## Call Signature

```ts
function seeder<R extends ResourceDeclaration<string, unknown>[] = ResourceDeclaration<string, unknown>[]>(options: {
  resources?: R;
}, run: (ctx: SeederContext<R>) => unknown): SeederDeclaration;
```

Declare a seeder (the default export of a file matched by the
module's `seeders` globs). `usai db seed [name]` runs it as finite work
with the declared resources.

### Type Parameters

| Type Parameter | Default type |
| ------ | ------ |
| `R` *extends* [`ResourceDeclaration`](../interfaces/ResourceDeclaration.md)\<`string`, `unknown`\>[] | [`ResourceDeclaration`](../interfaces/ResourceDeclaration.md)\<`string`, `unknown`\>[] |

### Parameters

| Parameter | Type |
| ------ | ------ |
| `options` | \{ `resources?`: `R`; \} |
| `options.resources?` | `R` |
| `run` | (`ctx`: [`SeederContext`](../interfaces/SeederContext.md)\<`R`\>) => `unknown` |

### Returns

[`SeederDeclaration`](../interfaces/SeederDeclaration.md)

## Call Signature

```ts
function seeder(run: (ctx: SeederContext) => unknown): SeederDeclaration;
```

Declare a seeder (the default export of a file matched by the
module's `seeders` globs). `usai db seed [name]` runs it as finite work
with the declared resources.

### Parameters

| Parameter | Type |
| ------ | ------ |
| `run` | (`ctx`: [`SeederContext`](../interfaces/SeederContext.md)) => `unknown` |

### Returns

[`SeederDeclaration`](../interfaces/SeederDeclaration.md)
