[@sakaladev/usai](../../README.md) / [index](../README.md) / parseMultipart

# Function: parseMultipart()

```ts
function parseMultipart(body: Uint8Array, contentType: string | undefined): MultipartBody;
```

Parses a `multipart/form-data` body. `contentType` is the request's
`content-type` header (the boundary is read from it). Throws a
`TypeError` on a malformed body — answer 400 with it.

## Parameters

| Parameter | Type |
| ------ | ------ |
| `body` | `Uint8Array` |
| `contentType` | `string` \| `undefined` |

## Returns

[`MultipartBody`](../interfaces/MultipartBody.md)

## Example

```ts
export const upload = http.raw("/notes/:id/attachment", { method: "POST", auth, resources: [db] }, async (ctx) => {
  const form = multipart.parse(await ctx.request.bytes(), ctx.headers["content-type"]);
  const file = form.files[0];
  if (!file) throw errors.custom("validation_failed", 400, "one file expected");
  await ctx.resources.db.execute("insert into attachments (note_id, type, data) values ($1, $2, $3)", [ctx.params.id, file.contentType, file.data]);
  return http.rawResponse(201, JSON.stringify({ size: file.data.length }), { "content-type": "application/json" });
});
```
