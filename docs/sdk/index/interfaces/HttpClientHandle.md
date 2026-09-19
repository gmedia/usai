[@sakaladev/usai](../../README.md) / [index](../README.md) / HttpClientHandle

# Interface: HttpClientHandle

The in-world handle for an `http.client` resource (`ctx.resources.<name>`).

## Methods

### fetch()

```ts
fetch(url: string, init?: FetchInit): Promise<FetchResponse>;
```

One request. `url` is a path (relative to `baseUrl`) or an absolute
URL on the pinned origin.

#### Parameters

| Parameter | Type |
| ------ | ------ |
| `url` | `string` |
| `init?` | [`FetchInit`](FetchInit.md) |

#### Returns

`Promise`\<[`FetchResponse`](FetchResponse.md)\>
