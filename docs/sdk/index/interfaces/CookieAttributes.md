[@sakaladev/usai](../../README.md) / [index](../README.md) / CookieAttributes

# Interface: CookieAttributes

Attributes of a `Set-Cookie` value. Secure, HttpOnly and `SameSite=Lax`
are the defaults: a session cookie that a script can read or that travels
over http is the exception, and has to be asked for.

## Properties

| Property | Type | Description |
| ------ | ------ | ------ |
| <a id="maxage"></a> `maxAge?` | `number` | Seconds until expiry; `0` deletes the cookie (`Max-Age=0`). |
| <a id="expires"></a> `expires?` | `Date` | - |
| <a id="path"></a> `path?` | `string` | Default `/`. |
| <a id="domain"></a> `domain?` | `string` | - |
| <a id="secure"></a> `secure?` | `boolean` | Default `true`. |
| <a id="httponly"></a> `httpOnly?` | `boolean` | Default `true`. |
| <a id="samesite"></a> `sameSite?` | `"Strict"` \| `"Lax"` \| `"None"` | Default `"Lax"`. `"None"` requires `secure`. |
