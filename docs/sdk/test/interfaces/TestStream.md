[@sakaladev/usai](../../README.md) / [test](../README.md) / TestStream

# Interface: TestStream

An open event stream.

## Properties

| Property | Modifier | Type |
| ------ | ------ | ------ |
| <a id="status"></a> `status` | `readonly` | `number` |
| <a id="headers"></a> `headers` | `readonly` | `Record`\<`string`, `string`\> |

## Methods

### next()

```ts
next(timeoutMs?: number): Promise<SseEvent | null>;
```

The next event, or `null` when the stream ended; rejects after `timeoutMs` (default 5 000).

#### Parameters

| Parameter | Type |
| ------ | ------ |
| `timeoutMs?` | `number` |

#### Returns

`Promise`\<[`SseEvent`](SseEvent.md) \| `null`\>

***

### text()

```ts
text(): Promise<string>;
```

The whole body as text, for a stream that is not SSE (a CSV download). Waits for the end.

#### Returns

`Promise`\<`string`\>

***

### close()

```ts
close(): void;
```

Closes the connection — what a browser does when the tab goes; the world is cancelled.

#### Returns

`void`
