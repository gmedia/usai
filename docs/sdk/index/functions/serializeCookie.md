[@sakaladev/usai](../../README.md) / [index](../README.md) / serializeCookie

# Function: serializeCookie()

```ts
function serializeCookie(
   name: string, 
   value: string, 
   attributes?: CookieAttributes
): string;
```

Serialises one `Set-Cookie` header value. The value is percent-encoded
where the cookie grammar requires it, so anything round-trips through
`parseCookies`.

## Parameters

| Parameter | Type |
| ------ | ------ |
| `name` | `string` |
| `value` | `string` |
| `attributes` | [`CookieAttributes`](../interfaces/CookieAttributes.md) |

## Returns

`string`

## Example

```ts
http.response(200, body, { "set-cookie": serializeCookie("sid", token, { maxAge: 86_400 }) });
http.noContent({ "set-cookie": serializeCookie("sid", "", { maxAge: 0 }) });   // logout
```
