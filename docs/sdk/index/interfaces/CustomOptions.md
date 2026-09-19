[@sakaladev/usai](../../README.md) / [index](../README.md) / CustomOptions

# Interface: CustomOptions\<P\>

Options for `auth.custom`.

## Type Parameters

| Type Parameter |
| ------ |
| `P` |

## Properties

| Property | Type | Description |
| ------ | ------ | ------ |
| <a id="name"></a> `name` | `string` | - |
| <a id="resolve"></a> `resolve` | (`ctx`: [`BaseContext`](BaseContext.md) & \{ `request`: [`AuthRequest`](AuthRequest.md); \}) => `P` \| `Promise`\<`P`\> | Inspect `ctx.request` (headers, query) and return the principal, or throw. |
