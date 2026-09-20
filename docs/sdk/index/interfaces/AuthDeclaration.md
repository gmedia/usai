[@sakaladev/usai](../../README.md) / [index](../README.md) / AuthDeclaration

# Interface: AuthDeclaration\<Principal = `unknown`\>

What `auth.bearer`/`auth.header`/`auth.custom` return: a named
boundary reused by reference. `Principal` is the type of `ctx.auth`.

## Type Parameters

| Type Parameter | Default type |
| ------ | ------ |
| `Principal` | `unknown` |

## Properties

| Property | Modifier | Type | Description |
| ------ | ------ | ------ | ------ |
| <a id="name"></a> `name` | `readonly` | `string` | - |
| <a id="description"></a> `description?` | `readonly` | `string` | For the OpenAPI security scheme: where the credential comes from. |
| <a id="scheme"></a> `scheme` | `readonly` | `"bearer"` \| `"header"` \| `"cookie"` \| `"custom"` | - |
| <a id="header"></a> `header?` | `readonly` | `string` | - |
| <a id="credential"></a> `credential?` | `readonly` | [`CredentialLocation`](CredentialLocation.md) | For a custom scheme: where the credential travels, so the OpenAPI document and the reference describe it truthfully (a cookie, a query parameter, a header). Without it the document says only that the resolver reads the request. |
| <a id="resources"></a> `resources` | `readonly` | readonly [`ResourceDeclaration`](ResourceDeclaration.md)\<`string`, `unknown`\>[] | The resources the resolver leases. A workload that uses the scheme gets them in addition to its own (`describe` merges the two lists), so a route never has to repeat the session table for the resolver's sake. |
| <a id="resolve"></a> `resolve` | `readonly` | (`ctx`: `unknown`, `credential`: `string` \| `undefined`) => `Principal` \| `Promise`\<`Principal`\> | - |
