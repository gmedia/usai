[@sakaladev/usai](../../README.md) / [index](../README.md) / RawRequestBody

# Interface: RawRequestBody

The exact bytes of a raw request, decoded on demand: `await ctx.request.bytes()`,
`await ctx.request.text()`, or `await ctx.request.json()`. Each is a method
(the body is not read until asked for).

## Methods

### bytes()

```ts
bytes(): Promise<Uint8Array<ArrayBufferLike>>;
```

The body bytes, exactly as received.

#### Returns

`Promise`\<`Uint8Array`\<`ArrayBufferLike`\>\>

***

### text()

```ts
text(): Promise<string>;
```

The body decoded as UTF-8.

#### Returns

`Promise`\<`string`\>

***

### json()

```ts
json(): Promise<unknown>;
```

The body parsed as JSON (throws on invalid JSON).

#### Returns

`Promise`\<`unknown`\>
