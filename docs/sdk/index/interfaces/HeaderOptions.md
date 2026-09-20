[@sakaladev/usai](../../README.md) / [index](../README.md) / HeaderOptions

# Interface: HeaderOptions\<P, R = [`ResourceDeclaration`](ResourceDeclaration.md)[]\>

Options for `auth.header`.

## Type Parameters

| Type Parameter | Default type |
| ------ | ------ |
| `P` | - |
| `R` | [`ResourceDeclaration`](ResourceDeclaration.md)[] |

## Properties

| Property | Type | Description |
| ------ | ------ | ------ |
| <a id="name"></a> `name?` | `string` | Scheme name. Default `header-<n>`. |
| <a id="description"></a> `description?` | `string` | Where the credential comes from, for the reference. |
| <a id="header"></a> `header` | `string` | The header carrying the credential (case-insensitive). |
| <a id="resources"></a> `resources?` | `R` | The resources the resolver leases; every workload using the scheme gets them. |
| <a id="resolve"></a> `resolve` | (`ctx`: [`ResolverContext`](../type-aliases/ResolverContext.md)\<`R`\>, `value`: `string`) => `P` \| `Promise`\<`P`\> | Return the principal, or throw `errors.unauthorized()`. |
