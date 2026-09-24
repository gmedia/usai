[@sakaladev/usai](../../README.md) / [test](../README.md) / TestAppOptions

# Interface: TestAppOptions

Options for [testApp](../functions/testApp.md).

## Properties

| Property | Type | Description |
| ------ | ------ | ------ |
| <a id="root"></a> `root?` | `string` | Project root (directory with usai.config.ts / src/app.ts). Default: cwd. |
| <a id="binary"></a> `binary?` | `string` | Path to the `usai` binary. Default: $USAI_BIN, then `usai` on PATH. |
| <a id="env"></a> `env?` | `Record`\<`string`, `string`\> | Environment for the runtime (merged over process.env). |
| <a id="starttimeoutms"></a> `startTimeoutMs?` | `number` | Milliseconds to wait for the runtime to announce itself. Default 60000. |
| <a id="args"></a> `args?` | `string`[] | Extra CLI arguments (e.g. ["--status"]). |
| <a id="migrate"></a> `migrate?` | \| `boolean` \| \{ `seed?`: `string` \| `boolean`; \} | Run `usai db migrate` (and optionally `usai db seed`) against the configured database before starting — for tests on a throwaway database. Migrations are never run at startup by the runtime itself. |
| <a id="schedulers"></a> `schedulers?` | \| `boolean` \| \{ `cron?`: `boolean`; `queue?`: `boolean`; `services?`: `boolean`; \} | Run the background schedulers: cron, queue consumers and services. **Off by default**, because a test run is supposed to be deterministic and they are not: a `* * * * *` schedule fires in the middle of a test from the wall clock, and — since `usai test` runs test files in parallel against one database — two files' consumers take each other's queue messages, so a retry test sees four attempts and its neighbour sees two. `app.cron(name).run()`, `app.queue(topic).deliver(msg)` and `app.task(name).invoke()` drive that work explicitly instead, one delivery at a time. Turn them on for the tests that need the real scheduler — retry, backoff and dead-lettering end to end — and give those a database of their own (`env: { DATABASE_URL }`). |
