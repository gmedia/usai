[@sakaladev/usai](../../README.md) / [test](../README.md) / TestSocket

# Interface: TestSocket

An open WebSocket.

## Properties

| Property | Modifier | Type | Description |
| ------ | ------ | ------ | ------ |
| <a id="protocol"></a> `protocol` | `readonly` | `string` \| `undefined` | The subprotocol the server chose, if any. |

## Methods

### send()

```ts
send(message: unknown): Promise<void>;
```

Sends one text frame: a string as is, anything else as JSON.

#### Parameters

| Parameter | Type |
| ------ | ------ |
| `message` | `unknown` |

#### Returns

`Promise`\<`void`\>

***

### next()

```ts
next<T = unknown>(timeoutMs?: number): Promise<T | null>;
```

The next text frame (parsed as JSON when it is JSON, else the string), or `null` once closed; rejects after `timeoutMs` (default 5 000).

#### Type Parameters

| Type Parameter | Default type |
| ------ | ------ |
| `T` | `unknown` |

#### Parameters

| Parameter | Type |
| ------ | ------ |
| `timeoutMs?` | `number` |

#### Returns

`Promise`\<`T` \| `null`\>

***

### closed()

```ts
closed(timeoutMs?: number): Promise<{
  code: number;
  reason: string;
}>;
```

Waits for the server's close frame: `{ code, reason }`.

#### Parameters

| Parameter | Type |
| ------ | ------ |
| `timeoutMs?` | `number` |

#### Returns

`Promise`\<\{
  `code`: `number`;
  `reason`: `string`;
\}\>

***

### close()

```ts
close(code?: number, reason?: string): Promise<void>;
```

Sends a close frame and ends the connection.

#### Parameters

| Parameter | Type |
| ------ | ------ |
| `code?` | `number` |
| `reason?` | `string` |

#### Returns

`Promise`\<`void`\>
