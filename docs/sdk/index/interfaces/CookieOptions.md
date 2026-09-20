[@sakaladev/usai](../../README.md) / [index](../README.md) / CookieOptions

# Interface: CookieOptions\<P, R = [`ResourceDeclaration`](ResourceDeclaration.md)[]\>

Options for `auth.cookie`.

## Type Parameters

| Type Parameter | Default type |
| ------ | ------ |
| `P` | - |
| `R` | [`ResourceDeclaration`](ResourceDeclaration.md)[] |

## Properties

| Property | Type | Description |
| ------ | ------ | ------ |
| <a id="name"></a> `name?` | `string` | - |
| <a id="description"></a> `description?` | `string` | - |
| <a id="cookie"></a> `cookie` | `string` | The cookie's name (`sid`). Its value is what `resolve` receives; a signed value is verified in the resolver with `cookies.verify`. |
| <a id="resources"></a> `resources?` | `R` | The resources the resolver leases; every workload using the scheme gets them. |
| <a id="resolve"></a> `resolve` | (`ctx`: [`ResolverContext`](../type-aliases/ResolverContext.md)\<`R`\>, `value`: `string`) => `P` \| `Promise`\<`P`\> | Return the principal, or throw `errors.unauthorized()`. |
