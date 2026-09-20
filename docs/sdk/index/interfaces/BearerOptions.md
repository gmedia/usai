[@sakaladev/usai](../../README.md) / [index](../README.md) / BearerOptions

# Interface: BearerOptions\<P, R = [`ResourceDeclaration`](ResourceDeclaration.md)[]\>

Options for `auth.bearer`.

## Type Parameters

| Type Parameter | Default type |
| ------ | ------ |
| `P` | - |
| `R` | [`ResourceDeclaration`](ResourceDeclaration.md)[] |

## Properties

| Property | Type | Description |
| ------ | ------ | ------ |
| <a id="name"></a> `name?` | `string` | Scheme name (the OpenAPI security scheme and what Try it remembers). Default `bearer-<n>`. |
| <a id="description"></a> `description?` | `string` | For the OpenAPI security scheme and the reference: where the credential comes from. |
| <a id="resources"></a> `resources?` | `R` | The resources the resolver leases (a sessions table). Every workload that uses the scheme gets them, in addition to its own `resources`, and `ctx.resources` in `resolve` is typed by them. |
| <a id="resolve"></a> `resolve` | (`ctx`: [`ResolverContext`](../type-aliases/ResolverContext.md)\<`R`\>, `token`: `string`) => `P` \| `Promise`\<`P`\> | Return the principal, or throw `errors.unauthorized()`. |
