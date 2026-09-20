[@sakaladev/usai](../../README.md) / [index](../README.md) / parseCookies

# Function: parseCookies()

```ts
function parseCookies(header: string | null | undefined): Record<string, string>;
```

Parses a `cookie` request header into a name → value map (the first
occurrence of a name wins, as browsers order them by specificity).
Values are percent-decoded when they decode; a malformed pair is skipped.

## Parameters

| Parameter | Type |
| ------ | ------ |
| `header` | `string` \| `null` \| `undefined` |

## Returns

`Record`\<`string`, `string`\>
