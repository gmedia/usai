[@sakaladev/usai](../../README.md) / [index](../README.md) / WorkloadPolicies

# Interface: WorkloadPolicies

Bounds every workload can declare.

## Extended by

- [`HttpOptions`](HttpOptions.md)
- [`RawOptions`](RawOptions.md)
- [`TaskOptions`](TaskOptions.md)
- [`CronOptions`](CronOptions.md)
- [`StreamOptions`](StreamOptions.md)
- [`SocketOptions`](SocketOptions.md)
- [`ConsumeOptions`](ConsumeOptions.md)

## Properties

| Property | Type | Description |
| ------ | ------ | ------ |
| <a id="timeout"></a> `timeout?` | `string` \| `number` | Per-invocation deadline (`"5s"`, `"500ms"`, or milliseconds). The world is cancelled when it passes; HTTP callers get 504. Undeclared: the runtime default (30 s) for requests, none for the other kinds. |
| <a id="concurrency"></a> `concurrency?` | `number` | How many worlds of this workload may run at once; the next request is refused with 503 `capacity_exhausted`, not queued (ADR-0012). |
