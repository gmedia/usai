[@sakaladev/usai](../../README.md) / [index](../README.md) / seeder

# Function: seeder()

## Call Signature

```ts
function seeder(options: {
  resources?: ResourceDeclaration[];
}, run: (ctx: SeederContext) => unknown): SeederDeclaration;
```

Declare a seeder (the default export of a file matched by the
module's `seeders` globs). `usai db seed [name]` runs it as finite work
with the declared resources.

### Parameters

| Parameter | Type |
| ------ | ------ |
| `options` | \{ `resources?`: [`ResourceDeclaration`](../interfaces/ResourceDeclaration.md)[]; \} |
| `options.resources?` | [`ResourceDeclaration`](../interfaces/ResourceDeclaration.md)[] |
| `run` | (`ctx`: [`SeederContext`](../interfaces/SeederContext.md)) => `unknown` |

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
