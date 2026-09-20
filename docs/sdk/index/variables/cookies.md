[@sakaladev/usai](../../README.md) / [index](../README.md) / cookies

# Variable: cookies

```ts
const cookies: {
  parse: (header: string | null | undefined) => Record<string, string>;
  serialize: (name: string, value: string, attributes: CookieAttributes) => string;
  sign: (value: string, secret: string) => Promise<string>;
  verify: (signed: string | null | undefined, ...secrets: string[]) => Promise<string | null>;
};
```

The cookie helpers as one object, for `import { cookies } from "@sakaladev/usai"`.

## Type Declaration

## Authentication

| Name | Type | Default value |
| ------ | ------ | ------ |
| <a id="property-parse"></a> `parse()` | (`header`: `string` \| `null` \| `undefined`) => `Record`\<`string`, `string`\> | `parseCookies` |
| <a id="property-serialize"></a> `serialize()` | (`name`: `string`, `value`: `string`, `attributes`: [`CookieAttributes`](../interfaces/CookieAttributes.md)) => `string` | `serializeCookie` |
| <a id="property-sign"></a> `sign()` | (`value`: `string`, `secret`: `string`) => `Promise`\<`string`\> | `signCookieValue` |
| <a id="property-verify"></a> `verify()` | (`signed`: `string` \| `null` \| `undefined`, ...`secrets`: `string`[]) => `Promise`\<`string` \| `null`\> | `verifyCookieValue` |
