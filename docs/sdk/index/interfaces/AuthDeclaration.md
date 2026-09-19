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
| <a id="scheme"></a> `scheme` | `readonly` | `"bearer"` \| `"header"` \| `"custom"` | - |
| <a id="header"></a> `header?` | `readonly` | `string` | - |
| <a id="resolve"></a> `resolve` | `readonly` | (`ctx`: `unknown`, `credential`: `string` \| `undefined`) => `Principal` \| `Promise`\<`Principal`\> | - |
