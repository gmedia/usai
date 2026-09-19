[@sakaladev/usai](../../../../README.md) / [index](../../../README.md) / [StandardSchemaV1](../README.md) / InferInput

# Type Alias: InferInput\<S *extends* [`StandardSchemaV1`](../../../interfaces/StandardSchemaV1.md)\>

```ts
type InferInput<S extends StandardSchemaV1> = NonNullable<S["~standard"]["types"]>["input"];
```

## Type Parameters

| Type Parameter |
| ------ |
| `S` *extends* [`StandardSchemaV1`](../../../interfaces/StandardSchemaV1.md) |
