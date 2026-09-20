[@sakaladev/usai](../../README.md) / [index](../README.md) / AuthRequest

# Interface: AuthRequest

What an auth resolver sees of the request: method, path, headers and
query — never the body. The resolver runs **inside the request's world**,
after the boundary validated the request and before the handler
(ADR-0004): a session lookup is an ordinary query on the scheme's own
`resources: [db]` (typed on `ctx.resources`; every workload that uses the
scheme leases them too), and `ctx.env` is the application's.

## Properties

| Property | Modifier | Type |
| ------ | ------ | ------ |
| <a id="method"></a> `method` | `readonly` | `string` |
| <a id="path"></a> `path` | `readonly` | `string` |
| <a id="headers"></a> `headers` | `readonly` | `Record`\<`string`, `string`\> |
| <a id="query"></a> `query` | `readonly` | `Record`\<`string`, `string` \| `string`[]\> |
