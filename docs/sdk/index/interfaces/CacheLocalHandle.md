[@sakaladev/usai](../../README.md) / [index](../README.md) / CacheLocalHandle

# Interface: CacheLocalHandle

The in-world handle for a `cache.local` resource (`ctx.resources.<name>`).

## Methods

### get()

```ts
get<T = unknown>(key: string): Promise<T | null>;
```

The value, or `null` when absent or expired.

#### Type Parameters

| Type Parameter | Default type |
| ------ | ------ |
| `T` | `unknown` |

#### Parameters

| Parameter | Type |
| ------ | ------ |
| `key` | `string` |

#### Returns

`Promise`\<`T` \| `null`\>

***

### set()

```ts
set(
   key: string, 
   value: unknown, 
   options?: {
  ttlMs?: number;
}
): Promise<boolean>;
```

Store a JSON value, optionally for `ttlMs`.

#### Parameters

| Parameter | Type |
| ------ | ------ |
| `key` | `string` |
| `value` | `unknown` |
| `options?` | \{ `ttlMs?`: `number`; \} |
| `options.ttlMs?` | `number` |

#### Returns

`Promise`\<`boolean`\>

***

### delete()

```ts
delete(key: string): Promise<boolean>;
```

Remove a key; `true` when something was removed.

#### Parameters

| Parameter | Type |
| ------ | ------ |
| `key` | `string` |

#### Returns

`Promise`\<`boolean`\>

***

### increment()

```ts
increment(key: string, by?: number): Promise<number>;
```

Atomically add `by` (default 1) and return the new value; a missing key starts at 0.

#### Parameters

| Parameter | Type |
| ------ | ------ |
| `key` | `string` |
| `by?` | `number` |

#### Returns

`Promise`\<`number`\>

***

### clear()

```ts
clear(): Promise<boolean>;
```

Remove every entry.

#### Returns

`Promise`\<`boolean`\>
