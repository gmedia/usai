[@sakaladev/usai](../../README.md) / [index](../README.md) / AuthDeclaration

# Interface: AuthDeclaration\<Principal = `unknown`\>

What `auth.bearer`/`auth.header`/`auth.custom` return: a named
boundary reused by reference. `Principal` is the type of `ctx.auth`.

## Type Parameters

| Type Parameter | Default type |
| ------ | ------ |
| `Principal` | `unknown` |

## Properties

| Property | Modifier | Type |
| ------ | ------ | ------ |
| <a id="__usai"></a> `__usai` | `readonly` | `"auth"` |
| <a id="name"></a> `name` | `readonly` | `string` |
| <a id="scheme"></a> `scheme` | `readonly` | `"bearer"` \| `"header"` \| `"custom"` |
| <a id="header"></a> `header?` | `readonly` | `string` |
| <a id="resolve"></a> `resolve` | `readonly` | (`ctx`: `unknown`, `credential`: `string` \| `undefined`) => `Principal` \| `Promise`\<`Principal`\> |
