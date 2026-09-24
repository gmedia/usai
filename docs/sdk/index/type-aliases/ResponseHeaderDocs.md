[@sakaladev/usai](../../README.md) / [index](../README.md) / ResponseHeaderDocs

# Type Alias: ResponseHeaderDocs

```ts
type ResponseHeaderDocs = Partial<Record<number | "*", Record<string, string>>>;
```

Response headers an endpoint sets, documented per status for the
reference and the OpenAPI document (`responses[status].headers`): the
key is the status (`201`, `200`, or `"*"` for every status), the value
maps a header name to one line about it. Descriptive — the runtime does
not validate them; a generated client learns they exist.

## Example

```ts
responseHeaders: { 201: { location: "URL of the new user" }, "*": { etag: "Version of the resource" } }
```
