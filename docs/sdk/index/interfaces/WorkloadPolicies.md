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
| <a id="timeout"></a> `timeout?` | `string` \| `number` | Per-invocation deadline (`"5s"`, `"500ms"`, or milliseconds). For **finite** work the world is cancelled when it passes: an HTTP caller gets 504, an `invoke` rejects with `deadline_exceeded`, a queue message counts as a failed attempt. Undeclared, finite work gets the runtime default of 30 s. For **connection-bound and persistent** work — a stream, a socket, a service — there is **no default**, because one that ended after 30 s would be useless. A deadline you declare is honoured, and it **stops** the world rather than cancelling it: `ctx.signal` aborts, a pending `ctx.sleep` returns, the handler can finish what it is doing, and a socket's connection is closed so its `close` handler runs. For a stream the client keeps the `200` it already has and the body stops there — with no trailer and no error, so end an export with a sentinel the reader requires. |
| <a id="concurrency"></a> `concurrency?` | `number` | How many worlds of this workload may run at once. Past the bound the next one is refused, never queued: an HTTP request gets 503 `capacity_exhausted`, `ctx.tasks.invoke`/`dispatch` reject with the same code in the caller's world, a queue consumer simply claims fewer messages (ADR-0012). |
| <a id="maxbodybytes"></a> `maxBodyBytes?` | `number` | Request body bound for this route, in bytes. A **cap**, never a raise: the effective bound is the smaller of this and the process's `USAI_MAX_BODY_BYTES` (1 MiB by default), so the operator keeps the ceiling and each route decides how much of it to accept. Declare a small one on ordinary routes and raise the process bound for the one that takes uploads, instead of opening every route to the largest body any of them needs. Above the bound the request is `413 payload_too_large`, decided before a world exists. |
