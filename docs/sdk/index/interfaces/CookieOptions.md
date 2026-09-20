[@sakaladev/usai](../../README.md) / [index](../README.md) / CookieOptions

# Interface: CookieOptions\<P\>

Options for `auth.cookie`.

## Type Parameters

| Type Parameter |
| ------ |
| `P` |

## Properties

| Property | Type | Description |
| ------ | ------ | ------ |
| <a id="name"></a> `name?` | `string` | - |
| <a id="description"></a> `description?` | `string` | - |
| <a id="cookie"></a> `cookie` | `string` | The cookie's name (`sid`). Its value is what `resolve` receives; a signed value is verified in the resolver with `cookies.verify`. |
| <a id="resolve"></a> `resolve` | (`ctx`: [`BaseContext`](BaseContext.md) & \{ `request`: [`AuthRequest`](AuthRequest.md); \}, `value`: `string`) => `P` \| `Promise`\<`P`\> | Return the principal, or throw `errors.unauthorized()`. |
