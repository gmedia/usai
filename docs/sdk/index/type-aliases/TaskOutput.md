[@sakaladev/usai](../../README.md) / [index](../README.md) / TaskOutput

# Type Alias: TaskOutput\<W\>

```ts
type TaskOutput<W> = W extends TypedWorkload<infer Out, infer _In> ? Out : unknown;
```

What `ctx.tasks.invoke` resolves to for a given task declaration.

## Type Parameters

| Type Parameter |
| ------ |
| `W` |
