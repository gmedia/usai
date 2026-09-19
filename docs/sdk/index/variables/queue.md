[@sakaladev/usai](../../README.md) / [index](../README.md) / queue

# Variable: queue

```ts
const queue: {
  consume: <M, R>(topic: string, options: ConsumeOptions<M, R>, handler: (ctx: QueueContext<M extends AnySchema ? Output<M> : unknown, R>) => unknown) => Workload;
};
```

Queue workloads: `queue.consume(topic, options, handler)`.

## Type Declaration

| Name | Type |
| ------ | ------ |
| <a id="property-consume"></a> `consume()` | \<`M`, `R`\>(`topic`: `string`, `options`: [`ConsumeOptions`](../interfaces/ConsumeOptions.md)\<`M`, `R`\>, `handler`: (`ctx`: [`QueueContext`](../interfaces/QueueContext.md)\<`M` *extends* [`AnySchema`](../type-aliases/AnySchema.md) ? [`Output`](../type-aliases/Output.md)\<`M`\> : `unknown`, `R`\>) => `unknown`) => [`Workload`](../interfaces/Workload.md) |
