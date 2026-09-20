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
| <a id="resolve"></a> `resolve` | `readonly` | (`ctx`: `unknown`, `credential`: `string` \| `undefined`) => `Principal` \| `Promise`\<`Principal`\> | - |
