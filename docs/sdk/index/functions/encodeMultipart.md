[@sakaladev/usai](../../README.md) / [index](../README.md) / encodeMultipart

# Function: encodeMultipart()

```ts
function encodeMultipart(parts: readonly MultipartPart[], boundary?: string): {
  body: Uint8Array;
  contentType: string;
};
```

Encodes a `multipart/form-data` body — what a browser form or `curl -F`
sends — for a test or an outbound call: `const { body, contentType } =
multipart.encode([{ name: "file", filename: "a.csv", data }])`, then
`app.http.post("/imports", { body, headers: { "content-type": contentType } })`.

## Parameters

| Parameter | Type |
| ------ | ------ |
| `parts` | readonly [`MultipartPart`](../type-aliases/MultipartPart.md)[] |
| `boundary` | `string` |

## Returns

```ts
{
  body: Uint8Array;
  contentType: string;
}
```

| Name | Type |
| ------ | ------ |
| `body` | `Uint8Array` |
| `contentType` | `string` |
