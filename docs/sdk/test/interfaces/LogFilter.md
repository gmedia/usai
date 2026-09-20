[@sakaladev/usai](../../README.md) / [test](../README.md) / LogFilter

# Interface: LogFilter

What `TestApp.logs` / `waitForLog` select on; every given field must match.

## Properties

| Property | Type | Description |
| ------ | ------ | ------ |
| <a id="requestid"></a> `requestId?` | `string` | - |
| <a id="workload"></a> `workload?` | `string` | - |
| <a id="level"></a> `level?` | `"TRACE"` \| `"DEBUG"` \| `"INFO"` \| `"WARN"` \| `"ERROR"` | - |
| <a id="target"></a> `target?` | `string` | `app` for the application's lines. |
| <a id="message"></a> `message?` | `string` \| `RegExp` | A substring of the message, or a pattern. |
| <a id="where"></a> `where?` | (`line`: [`LogLine`](LogLine.md)) => `boolean` | Any predicate over the parsed line. |
