[@sakaladev/usai](../../README.md) / [index](../README.md) / BearerOptions

# Interface: BearerOptions\<P\>

Options for `auth.bearer`.

## Type Parameters

| Type Parameter |
| ------ |
| `P` |

## Properties

| Property | Type | Description |
| ------ | ------ | ------ |
| <a id="name"></a> `name?` | `string` | Scheme name (the OpenAPI security scheme and what Try it remembers). Default `bearer-<n>`. |
| <a id="resolve"></a> `resolve` | (`ctx`: [`BaseContext`](BaseContext.md) & \{ `request`: [`AuthRequest`](AuthRequest.md); \}, `token`: `string`) => `P` \| `Promise`\<`P`\> | Return the principal, or throw `errors.unauthorized()`. |
