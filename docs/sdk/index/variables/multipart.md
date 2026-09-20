[@sakaladev/usai](../../README.md) / [index](../README.md) / multipart

# Variable: multipart

```ts
const multipart: {
  parse: (body: Uint8Array, contentType: string | undefined) => MultipartBody;
  encode: (parts: readonly MultipartPart[], boundary: string) => {
     body: Uint8Array;
     contentType: string;
  };
};
```

The multipart helper as one object.

## Type Declaration

## HTTP

| Name | Type | Default value |
| ------ | ------ | ------ |
| <a id="property-parse"></a> `parse()` | (`body`: `Uint8Array`, `contentType`: `string` \| `undefined`) => [`MultipartBody`](../interfaces/MultipartBody.md) | `parseMultipart` |
| <a id="property-encode"></a> `encode()` | (`parts`: readonly [`MultipartPart`](../type-aliases/MultipartPart.md)[], `boundary`: `string`) => \{ `body`: `Uint8Array`; `contentType`: `string`; \} | `encodeMultipart` |
