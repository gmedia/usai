[@sakaladev/usai](../../README.md) / [index](../README.md) / CredentialLocation

# Interface: CredentialLocation

Where a custom scheme's credential travels: `{ in: "cookie", name: "sid" }`,
`{ in: "header", name: "x-session" }`, `{ in: "query", name: "token" }`.

## Properties

| Property | Modifier | Type |
| ------ | ------ | ------ |
| <a id="in"></a> `in` | `readonly` | `"header"` \| `"cookie"` \| `"query"` |
| <a id="name"></a> `name` | `readonly` | `string` |
