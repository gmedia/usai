[@sakaladev/usai](../../README.md) / [test](../README.md) / TestApp

# Interface: TestApp

A running application under test; see [testApp](../functions/testApp.md).

## Properties

| Property | Modifier | Type | Description |
| ------ | ------ | ------ | ------ |
| <a id="url"></a> `url` | `readonly` | `string` | Base URL of the application listener. |
| <a id="controlurl"></a> `controlUrl` | `readonly` | `string` | Base URL of the control surface. |
| <a id="http"></a> `http` | `readonly` | \{ `get`: `Promise`\<[`TestResponse`](TestResponse.md)\>; `post`: `Promise`\<[`TestResponse`](TestResponse.md)\>; `put`: `Promise`\<[`TestResponse`](TestResponse.md)\>; `patch`: `Promise`\<[`TestResponse`](TestResponse.md)\>; `delete`: `Promise`\<[`TestResponse`](TestResponse.md)\>; `request`: `Promise`\<[`TestResponse`](TestResponse.md)\>; \} | HTTP requests to the application, with JSON in and out. |
| `http.get` | `public` | `Promise`\<[`TestResponse`](TestResponse.md)\> | - |
| `http.post` | `public` | `Promise`\<[`TestResponse`](TestResponse.md)\> | - |
| `http.put` | `public` | `Promise`\<[`TestResponse`](TestResponse.md)\> | - |
| `http.patch` | `public` | `Promise`\<[`TestResponse`](TestResponse.md)\> | - |
| `http.delete` | `public` | `Promise`\<[`TestResponse`](TestResponse.md)\> | - |
| `http.request` | `public` | `Promise`\<[`TestResponse`](TestResponse.md)\> | - |

## Methods

### task()

```ts
task(name: string): {
  invoke: Promise<WorkOutcome<T>>;
};
```

Run a task once in a fresh world and get its [WorkOutcome](WorkOutcome.md).

#### Parameters

| Parameter | Type |
| ------ | ------ |
| `name` | `string` |

#### Returns

```ts
{
  invoke: Promise<WorkOutcome<T>>;
}
```

| Name | Type |
| ------ | ------ |
| `invoke()` | (`input?`: `unknown`) => `Promise`\<[`WorkOutcome`](WorkOutcome.md)\<`T`\>\> |

***

### cron()

```ts
cron(name: string): {
  run: Promise<WorkOutcome<T>>;
};
```

Run one cron tick, without the clock.

#### Parameters

| Parameter | Type |
| ------ | ------ |
| `name` | `string` |

#### Returns

```ts
{
  run: Promise<WorkOutcome<T>>;
}
```

| Name | Type |
| ------ | ------ |
| `run()` | () => `Promise`\<[`WorkOutcome`](WorkOutcome.md)\<`T`\>\> |

***

### command()

```ts
command(name: string): {
  run: Promise<WorkOutcome<T>>;
};
```

Run a command with arguments.

#### Parameters

| Parameter | Type |
| ------ | ------ |
| `name` | `string` |

#### Returns

```ts
{
  run: Promise<WorkOutcome<T>>;
}
```

| Name | Type |
| ------ | ------ |
| `run()` | (`args?`: `string`[]) => `Promise`\<[`WorkOutcome`](WorkOutcome.md)\<`T`\>\> |

***

### queue()

```ts
queue(topic: string): {
  deliver: Promise<WorkOutcome<T>>;
};
```

Deliver one message to a topic's consumer directly — a fresh world,
`ctx.attempt` 1, no row in `usai_queue`, no retry: the way to test a
consumer's logic (idempotency: deliver the same message twice) without
publishing through the application or waiting for the scheduler.

#### Parameters

| Parameter | Type |
| ------ | ------ |
| `topic` | `string` |

#### Returns

```ts
{
  deliver: Promise<WorkOutcome<T>>;
}
```

| Name | Type |
| ------ | ------ |
| `deliver()` | (`message`: `unknown`) => `Promise`\<[`WorkOutcome`](WorkOutcome.md)\<`T`\>\> |

***

### stream()

```ts
stream(path: string, options?: RequestOptions): Promise<TestStream>;
```

Opens an event stream (`http.stream`, `text/event-stream`) and reads
it event by event: `const s = await app.stream("/events"); const first =
await s.next(); s.close()`. Headers (a cookie, a bearer token,
`last-event-id`) go in `options.headers`. The response's status and
headers are known once the promise resolves; a non-2xx status resolves
too (read `status`/`text` instead of `next`).

#### Parameters

| Parameter | Type |
| ------ | ------ |
| `path` | `string` |
| `options?` | [`RequestOptions`](RequestOptions.md) |

#### Returns

`Promise`\<[`TestStream`](TestStream.md)\>

***

### socket()

```ts
socket(path: string, options?: SocketOptions): Promise<TestSocket>;
```

Opens a WebSocket (`socket(...)`) with the headers a browser would send —
a `cookie`, or `["bearer", token]` as `protocols` — and exchanges JSON
messages: `const ws = await app.socket("/chat", { headers: { cookie } });
await ws.send({ text: "hi" }); const reply = await ws.next(); await ws.close()`.
A refused credential rejects with the HTTP status (`TestSocketRefused`,
`status` 401) before any frame.

#### Parameters

| Parameter | Type |
| ------ | ------ |
| `path` | `string` |
| `options?` | [`SocketOptions`](SocketOptions.md) |

#### Returns

`Promise`\<[`TestSocket`](TestSocket.md)\>

***

### status()

```ts
status(): Promise<Record<string, unknown>>;
```

Runtime status JSON (`/status` on the control surface).

#### Returns

`Promise`\<`Record`\<`string`, `unknown`\>\>

***

### logs()

```ts
logs(filter?: LogFilter): LogLine[];
```

The runtime's log lines so far (the last 10 000), newest last —
everything the application wrote with `console.*`/`ctx.log.*` (target
`app`, at INFO) and the runtime's own WARN/ERROR lines. Filter by the
request id a response carried (`res.headers["x-request-id"]`) to see
what a request did, **including the tasks it dispatched**: the id
follows the hand-off.

#### Parameters

| Parameter | Type |
| ------ | ------ |
| `filter?` | [`LogFilter`](LogFilter.md) |

#### Returns

[`LogLine`](LogLine.md)[]

***

### waitForLog()

```ts
waitForLog(filter: LogFilter, timeoutMs?: number): Promise<LogLine>;
```

Waits for a log line matching `filter` (already written or arriving
within `timeoutMs`, default 5 000) — how a test observes a dispatched
task, which runs after the response was sent. Rejects on timeout with
the lines seen so far.

#### Parameters

| Parameter | Type |
| ------ | ------ |
| `filter` | [`LogFilter`](LogFilter.md) |
| `timeoutMs?` | `number` |

#### Returns

`Promise`\<[`LogLine`](LogLine.md)\>

***

### close()

```ts
close(): Promise<void>;
```

Stop the runtime (drains, then exits). Always call it, in `after`.

#### Returns

`Promise`\<`void`\>
