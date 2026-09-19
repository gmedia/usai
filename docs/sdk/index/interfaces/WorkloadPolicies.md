[@sakaladev/usai](../../README.md) / [index](../README.md) / WorkloadPolicies

# Interface: WorkloadPolicies

Bounds every workload can declare.

## Extended by

- [`HttpOptions`](HttpOptions.md)
- [`RawOptions`](RawOptions.md)
- [`TaskOptions`](TaskOptions.md)
- [`CronOptions`](CronOptions.md)
- [`CommandOptions`](CommandOptions.md)
- [`StreamOptions`](StreamOptions.md)
- [`SocketOptions`](SocketOptions.md)
- [`ConsumeOptions`](ConsumeOptions.md)

## Properties

| Property | Type | Description |
| ------ | ------ | ------ |
| <a id="timeout"></a> `timeout?` | `string` \| `number` | Per-invocation deadline (`"5s"`, `"500ms"`, or milliseconds). The world is cancelled when it passes: an HTTP caller gets 504, an `invoke` rejects with `deadline_exceeded`, a queue message counts as a failed attempt. Undeclared: the runtime default (30 s) for requests, none for the other kinds. |
| <a id="concurrency"></a> `concurrency?` | `number` | How many worlds of this workload may run at once. Past the bound the next one is refused, never queued: an HTTP request gets 503 `capacity_exhausted`, `ctx.tasks.invoke`/`dispatch` reject with the same code in the caller's world, a queue consumer simply claims fewer messages (ADR-0012). |
