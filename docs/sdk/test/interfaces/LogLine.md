[@sakaladev/usai](../../README.md) / [test](../README.md) / LogLine

# Interface: LogLine

One line of the runtime's JSON log, as `TestApp.logs` returns it.

## Indexable

```ts
[key: string]: unknown
```

Every other attribute the line carried.

## Properties

| Property | Type | Description |
| ------ | ------ | ------ |
| <a id="timestamp"></a> `timestamp` | `string` | - |
| <a id="level"></a> `level` | `"TRACE"` \| `"DEBUG"` \| `"INFO"` \| `"WARN"` \| `"ERROR"` | - |
| <a id="message"></a> `message` | `string` | - |
| <a id="target"></a> `target` | `string` | `app` for the application's own lines; the runtime module otherwise. |
| <a id="workload"></a> `workload?` | `string` | The workload id (`task:send-reset-email`) when a world wrote the line. |
| <a id="world"></a> `world?` | `string` | - |
| <a id="requestid"></a> `requestId?` | `string` | The request id the world ran under — the request's own, or the one handed to a task it invoked or dispatched. |
| <a id="fields"></a> `fields?` | `Record`\<`string`, `unknown`\> | Structured fields: the trailing object of `ctx.log.info("paid", { id })`. |
