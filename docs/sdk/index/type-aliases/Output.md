[@sakaladev/usai](../../README.md) / [index](../README.md) / Output

# Type Alias: Output\<S\>

```ts
type Output<S> = S extends StandardSchemaV1 ? InferOutput<S> : never;
```

The output type of a schema, as handlers see it.

## Type Parameters

| Type Parameter |
| ------ |
| `S` |
