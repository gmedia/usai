[@sakaladev/usai](../README.md) / test

# test

`@sakaladev/usai/test` — run the same application model tests will
meet in production. The harness spawns the `usai` runtime for the
project, talks HTTP to the application, and invokes tasks, cron ticks
and commands deterministically through the control surface: no wall
clock, no external server. Start with [testApp](functions/testApp.md).

## Testing

| Name | Description |
| ------ | ------ |
| [TestAppOptions](interfaces/TestAppOptions.md) | Options for [testApp](functions/testApp.md). |
| [TestResponse](interfaces/TestResponse.md) | One HTTP response from the application under test: `body` is the parsed JSON when the response was JSON, otherwise the text. |
| [RequestOptions](interfaces/RequestOptions.md) | Options for `app.http.*`. |
| [WorkOutcome](interfaces/WorkOutcome.md) | The result of a task, cron tick or command run through the control surface: the handler's value or its error, the world's termination, the lifecycle `violations` it committed (detached work, an open transaction) and its log lines. A test asserts on all of it. |
| [TestApp](interfaces/TestApp.md) | A running application under test; see [testApp](functions/testApp.md). |
| [LogLine](interfaces/LogLine.md) | One line of the runtime's JSON log, as `TestApp.logs` returns it. |
| [SseEvent](interfaces/SseEvent.md) | One server-sent event as `TestStream.next` returns it. |
| [TestStream](interfaces/TestStream.md) | An open event stream. |
| [SocketOptions](interfaces/SocketOptions.md) | Options for `TestApp.socket`. |
| [TestSocket](interfaces/TestSocket.md) | An open WebSocket. |
| [TestSocketRefused](classes/TestSocketRefused.md) | Thrown by `TestApp.socket` when the server answered the upgrade with an HTTP status instead of `101` (a refused credential is a `401`). |
| [LogFilter](interfaces/LogFilter.md) | What `TestApp.logs` / `waitForLog` select on; every given field must match. |
| [UsaiTestError](classes/UsaiTestError.md) | Thrown when the harness itself fails: the binary is missing, the runtime did not start, a control request was refused. |
| [testApp](functions/testApp.md) | Start the application under the real runtime for a test: the same artifact, the same boundaries, the same worlds production will run. The harness spawns the `usai` binary (`USAI_BIN`, `binary`, or `usai` on PATH) on random ports with a control token, waits for it to announce itself, and talks HTTP to the application and to the control surface. Tasks, cron ticks and commands run deterministically through the control surface — no wall clock, no external server — and answer with a [WorkOutcome](interfaces/WorkOutcome.md) that includes the world's lifecycle violations, so a test can fail on detached work the way production would. |

## Other

| Name | Description |
| ------ | ------ |
| [WorldLogLine](interfaces/WorldLogLine.md) | One line a world wrote, as the outcome carries it. |
| [Termination](type-aliases/Termination.md) | How a world ended. The four the runtime names, plus the open form so a future one is a value and not a type error. |
