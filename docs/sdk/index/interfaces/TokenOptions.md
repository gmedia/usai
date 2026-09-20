[@sakaladev/usai](../../README.md) / [index](../README.md) / TokenOptions

# Interface: TokenOptions

Options for `tokens.sign`.

## Properties

| Property | Type | Description |
| ------ | ------ | ------ |
| <a id="expiresin"></a> `expiresIn` | `string` \| `number` | Lifetime: `"15m"`, `"12h"`, or seconds as a number. Required — a bearer token without an expiry is a password. |
| <a id="now"></a> `now?` | `number` | `iat`/`exp` are computed from this instant (tests). Default `Date.now()`. |
