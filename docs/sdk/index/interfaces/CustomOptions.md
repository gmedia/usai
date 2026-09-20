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
| <a id="description"></a> `description?` | `string` | Where the credential comes from, for the reference. |
| <a id="credential"></a> `credential?` | [`CredentialLocation`](CredentialLocation.md) | Where the credential travels — `{ in: "cookie", name: "sid" }` for a session cookie. Declares it for the OpenAPI document (`apiKey` in that location) and the reference's request panel; the resolver still reads the request itself. Without it the document does not invent a header: the operation is marked authenticated with a custom scheme and no security scheme is emitted. |
| <a id="resolve"></a> `resolve` | (`ctx`: [`BaseContext`](BaseContext.md) & \{ `request`: [`AuthRequest`](AuthRequest.md); \}) => `P` \| `Promise`\<`P`\> | Inspect `ctx.request` (headers, query) and return the principal, or throw. |
