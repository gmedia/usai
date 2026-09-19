[@sakaladev/usai](../../../../README.md) / [index](../../../README.md) / [StandardSchemaV1](../README.md) / InferOutput

# Type Alias: InferOutput\<S *extends* [`StandardSchemaV1`](../../../interfaces/StandardSchemaV1.md)\>

```ts
type InferOutput<S extends StandardSchemaV1> = NonNullable<S["~standard"]["types"]>["output"];
```

## Type Parameters

| Type Parameter |
| ------ |
| `S` *extends* [`StandardSchemaV1`](../../../interfaces/StandardSchemaV1.md) |
