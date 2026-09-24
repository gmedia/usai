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
| <a id="termination"></a> `termination` | [`Termination`](../type-aliases/Termination.md) | How the world ended: `"completed"`, `"deadline-exceeded"`, `"cancelled: <reason>"` or `"faulted: <detail>"`. This is what a lifecycle test asserts on, and it used to be `unknown` — so asserting on it needed a cast, which is the defect this release fixed for `ctx.tasks.invoke`. The string form is open on purpose: a new termination must not fail to typecheck in an existing test. |
| <a id="durationms"></a> `durationMs` | `number` | - |
| <a id="violations"></a> `violations` | \{ `code`: `string`; `message`: `string`; \}[] | - |
| <a id="logs"></a> `logs` | [`WorldLogLine`](WorldLogLine.md)[] | Everything the world logged: the parsed `fields`, the workload and the request id included, so a test that has the outcome does not have to go back to `app.logs()` for them. (`app.logs()` adds the timestamp and the runtime's own lines; this is only what the world wrote.) |
