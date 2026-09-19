[@sakaladev/usai](../../README.md) / [index](../README.md) / service

# Function: service()

## Call Signature

```ts
function service(name: string, handler: (ctx: ServiceContext) => unknown): Workload;
```

Declare a service: the one **persistent** lifetime. One world starts
when the revision activates, runs the handler, and is asked to stop
(`ctx.signal` aborts) when the revision drains; a handler that ignores
the signal is cancelled at the drain bound. The handler returning or
throwing ends the service; `restart` decides what happens next. A
service is supervised per revision, so a replacement revision gets its
own instance and the old one stops with its revision.

### Parameters

| Parameter | Type |
| ------ | ------ |
| `name` | `string` |
| `handler` | (`ctx`: [`ServiceContext`](../interfaces/ServiceContext.md)) => `unknown` |

### Returns

[`Workload`](../interfaces/Workload.md)

### Example

```ts
export const ticker = service("ticker", { resources: [cache], restart: { mode: "on-failure" } }, async (ctx) => {
  while (!ctx.signal.aborted) {
    await ctx.resources.cache.increment("ticks");
    await ctx.sleep("1s");
  }
});
```

## Call Signature

```ts
function service(
   name: string, 
   options: ServiceOptions, 
   handler: (ctx: ServiceContext) => unknown
): Workload;
```

Declare a service: the one **persistent** lifetime. One world starts
when the revision activates, runs the handler, and is asked to stop
(`ctx.signal` aborts) when the revision drains; a handler that ignores
the signal is cancelled at the drain bound. The handler returning or
throwing ends the service; `restart` decides what happens next. A
service is supervised per revision, so a replacement revision gets its
own instance and the old one stops with its revision.

### Parameters

| Parameter | Type |
| ------ | ------ |
| `name` | `string` |
| `options` | [`ServiceOptions`](../interfaces/ServiceOptions.md) |
| `handler` | (`ctx`: [`ServiceContext`](../interfaces/ServiceContext.md)) => `unknown` |

### Returns

[`Workload`](../interfaces/Workload.md)

### Example

```ts
export const ticker = service("ticker", { resources: [cache], restart: { mode: "on-failure" } }, async (ctx) => {
  while (!ctx.signal.aborted) {
    await ctx.resources.cache.increment("ticks");
    await ctx.sleep("1s");
  }
});
```
