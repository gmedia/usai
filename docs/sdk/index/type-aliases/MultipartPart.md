[@sakaladev/usai](../../README.md) / [index](../README.md) / MultipartPart

# Type Alias: MultipartPart

```ts
type MultipartPart = 
  | {
  name: string;
  value: string;
}
  | {
  name: string;
  filename: string;
  contentType?: string;
  data: Uint8Array;
};
```

One part to encode: a text field, or a file with its bytes.
