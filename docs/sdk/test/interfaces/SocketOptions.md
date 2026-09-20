[@sakaladev/usai](../../README.md) / [test](../README.md) / SocketOptions

# Interface: SocketOptions

Options for `TestApp.socket`.

## Properties

| Property | Type | Description |
| ------ | ------ | ------ |
| <a id="headers"></a> `headers?` | `Record`\<`string`, `string`\> | Request headers for the upgrade (`cookie`, …). |
| <a id="protocols"></a> `protocols?` | readonly `string`[] | `Sec-WebSocket-Protocol` entries: `["bearer", token]` is how a browser passes a bearer token. |
