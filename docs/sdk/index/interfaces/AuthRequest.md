[@sakaladev/usai](../../README.md) / [index](../README.md) / AuthRequest

# Interface: AuthRequest

What an auth resolver sees of the request: method, path, headers and
query — never the body. The resolver runs **inside the request's world**,
after the boundary validated the request and before the handler
(ADR-0004): `ctx.resources` and `ctx.env` are the route's, so a session
lookup is an ordinary query, and a route that uses the scheme must list
the resources the resolver leases (`resources: [db]`) — a missing one is
`500 resource_not_declared` at the first request, not a compile error.
`ctx.resources` is untyped here (the scheme is declared once and reused by
many routes); cast to the handle type you declared.

## Properties

| Property | Modifier | Type |
| ------ | ------ | ------ |
| <a id="method"></a> `method` | `readonly` | `string` |
| <a id="path"></a> `path` | `readonly` | `string` |
| <a id="headers"></a> `headers` | `readonly` | `Record`\<`string`, `string`\> |
| <a id="query"></a> `query` | `readonly` | `Record`\<`string`, `string` \| `string`[]\> |
