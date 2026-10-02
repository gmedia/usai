[@sakaladev/usai](../../README.md) / [index](../README.md) / PostgresOptions

# Interface: PostgresOptions

Options for [postgres](../functions/postgres.md). The URL itself is never in the
declaration: it comes from the environment at activation.

## Properties

| Property | Type | Description |
| ------ | ------ | ------ |
| <a id="urlenv"></a> `urlEnv?` | `string` | Environment variable holding the connection URL. Default `DATABASE_URL`. |
| <a id="pool"></a> `pool?` | \{ `max?`: `number`; `recycling?`: `"clean"` \| `"fast"`; \} | - |
| `pool.max?` | `number` | Connections in the pool (default 16). A world that needs one while all are leased waits, bounded by its deadline. |
| `pool.recycling?` | `"clean"` \| `"fast"` | `clean` (default) resets session state on every checkout; `fast` skips that round-trip for applications that never touch session state. |
| <a id="tls"></a> `tls?` | \{ `caFile?`: `string`; \} | TLS is chosen by the URL's `sslmode` (`disable` | `prefer` | `require`) and the server certificate is always verified — against Mozilla's roots plus this PEM bundle (private CAs, managed-database roots). When unset, the runtime also honours `PGSSLROOTCERT` in the environment. |
| `tls.caFile?` | `string` | - |
| <a id="bigint"></a> `bigint?` | `"string"` \| `"number"` | How `bigint` (`int8`) columns come back. `"number"` (default): a number within ±2^53 and a string beyond it, because a JSON number would round it — so the type depends on the **value**, and a row typed `{ id: number }` is right until the day it is not. `"string"`: always a string, so the type follows the column; type those fields `string` and convert where arithmetic needs it (`BigInt(row.id)`). Parameters are unaffected: an int8 parameter accepts a number or a string either way. |
