[@sakaladev/usai](../../README.md) / [index](../README.md) / StreamHandle

# Interface: StreamHandle

The second argument of an `http.stream` handler: the response, chunk
by chunk. The first `send`/`event`/`start` **commits** the status and
headers; after that the world is bound to the connection and ends when
the handler returns, the client disconnects (the world is cancelled), or
the revision drains.

## Methods

### start()

```ts
start(options?: {
  status?: number;
  headers?: Record<string, string>;
}): Promise<void>;
```

Commit status/headers before the first chunk (optional).

#### Parameters

| Parameter | Type |
| ------ | ------ |
| `options?` | \{ `status?`: `number`; `headers?`: `Record`\<`string`, `string`\>; \} |
| `options.status?` | `number` |
| `options.headers?` | `Record`\<`string`, `string`\> |

#### Returns

`Promise`\<`void`\>

***

### send()

```ts
send(chunk: string | Uint8Array<ArrayBufferLike>): Promise<void>;
```

One chunk. Commits a 200 head on first use.

#### Parameters

| Parameter | Type |
| ------ | ------ |
| `chunk` | `string` \| `Uint8Array`\<`ArrayBufferLike`\> |

#### Returns

`Promise`\<`void`\>

***

### event()

```ts
event(name: string, data: unknown): Promise<void>;
```

Server-sent event: `event:` + `data:` lines.

#### Parameters

| Parameter | Type |
| ------ | ------ |
| `name` | `string` |
| `data` | `unknown` |

#### Returns

`Promise`\<`void`\>
