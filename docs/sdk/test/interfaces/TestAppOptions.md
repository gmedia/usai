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
