[@sakaladev/usai](../../README.md) / [test](../README.md) / WorkOutcome

# Interface: WorkOutcome\<T = `unknown`\>

The result of a task, cron tick or command run through the control
surface: the handler's value or its error, the world's termination, the
lifecycle `violations` it committed (detached work, an open transaction)
and its log lines. A test asserts on all of it.

## Type Parameters

| Type Parameter | Default type |
| ------ | ------ |
| `T` | `unknown` |

## Properties

| Property | Type | Description |
| ------ | ------ | ------ |
| <a id="ok"></a> `ok` | `boolean` | The handler returned (no throw, no violation). |
| <a id="value"></a> `value` | `T` | - |
| <a id="error"></a> `error` | \| \{ `name`: `string`; `message`: `string`; `usai?`: \{ `code`: `string`; `status`: `number`; `details?`: `unknown`; \}; \} \| `null` | - |
| <a id="termination"></a> `termination` | `unknown` | - |
| <a id="durationms"></a> `durationMs` | `number` | - |
| <a id="violations"></a> `violations` | \{ `code`: `string`; `message`: `string`; \}[] | - |
| <a id="logs"></a> `logs` | \{ `level`: `string`; `message`: `string`; \}[] | - |
