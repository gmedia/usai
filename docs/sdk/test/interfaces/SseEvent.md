[@sakaladev/usai](../../README.md) / [test](../README.md) / SseEvent

# Interface: SseEvent

One server-sent event as `TestStream.next` returns it.

## Properties

| Property | Type | Description |
| ------ | ------ | ------ |
| <a id="event"></a> `event` | `string` | The `event:` name; `"message"` when the frame had none. |
| <a id="data"></a> `data` | `string` | The `data:` lines joined with `\n`. |
| <a id="json"></a> `json` | `unknown` | `data` parsed as JSON when it is JSON, else `undefined`. |
| <a id="id"></a> `id?` | `string` | - |
| <a id="retry"></a> `retry?` | `number` | - |
