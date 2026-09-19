[@sakaladev/usai](../../README.md) / [index](../README.md) / HeaderOptions

# Interface: HeaderOptions\<P\>

Options for `auth.header`.

## Type Parameters

| Type Parameter |
| ------ |
| `P` |

## Properties

| Property | Type | Description |
| ------ | ------ | ------ |
| <a id="name"></a> `name?` | `string` | Scheme name. Default `header-<n>`. |
| <a id="header"></a> `header` | `string` | The header carrying the credential (case-insensitive). |
| <a id="resolve"></a> `resolve` | (`ctx`: [`BaseContext`](BaseContext.md) & \{ `request`: [`AuthRequest`](AuthRequest.md); \}, `value`: `string`) => `P` \| `Promise`\<`P`\> | Return the principal, or throw `errors.unauthorized()`. |
