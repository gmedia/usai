[@sakaladev/usai](../../README.md) / [index](../README.md) / PostgresHandle

# Interface: PostgresHandle

The in-world handle for a `postgres` resource. Rows are plain objects
keyed by column name; values are JSON (uuid/timestamps as strings). Each
statement leases its own connection; `transaction` pins one for the
callback and commits when it returns, rolls back when it throws. A world
that ends with the transaction still open is a lifecycle error, and the
runtime rolls back on its behalf.

## Extends

- [`SqlExecutor`](SqlExecutor.md)

## Methods

### query()

```ts
query<T = Record<string, unknown>>(sql: string, params?: SqlParam[]): Promise<T[]>;
```

Run a statement and return every row.

#### Type Parameters

| Type Parameter | Default type |
| ------ | ------ |
| `T` | `Record`\<`string`, `unknown`\> |

#### Parameters

| Parameter | Type |
| ------ | ------ |
| `sql` | `string` |
| `params?` | [`SqlParam`](../type-aliases/SqlParam.md)[] |

#### Returns

`Promise`\<`T`[]\>

#### Inherited from

[`SqlExecutor`](SqlExecutor.md).[`query`](SqlExecutor.md#query)

***

### one()

```ts
one<T = Record<string, unknown>>(sql: string, params?: SqlParam[]): Promise<T | null>;
```

Run a statement and return the first row, or `null`.

#### Type Parameters

| Type Parameter | Default type |
| ------ | ------ |
| `T` | `Record`\<`string`, `unknown`\> |

#### Parameters

| Parameter | Type |
| ------ | ------ |
| `sql` | `string` |
| `params?` | [`SqlParam`](../type-aliases/SqlParam.md)[] |

#### Returns

`Promise`\<`T` \| `null`\>

#### Inherited from

[`SqlExecutor`](SqlExecutor.md).[`one`](SqlExecutor.md#one)

***

### execute()

```ts
execute(sql: string, params?: SqlParam[]): Promise<number>;
```

Run a statement and return the number of rows affected.

#### Parameters

| Parameter | Type |
| ------ | ------ |
| `sql` | `string` |
| `params?` | [`SqlParam`](../type-aliases/SqlParam.md)[] |

#### Returns

`Promise`\<`number`\>

#### Inherited from

[`SqlExecutor`](SqlExecutor.md).[`execute`](SqlExecutor.md#execute)

***

### transaction()

```ts
transaction<T>(fn: (tx: SqlExecutor) => Promise<T>): Promise<T>;
```

One transaction on one pinned connection: `BEGIN`, the callback's
statements on `tx`, then `COMMIT` when it returns or `ROLLBACK` when it
throws (the error is rethrown). The transaction is live work owned by
this world: cancellation rolls it back, and a finite world that ends
with it still open is a lifecycle error, rolled back by the runtime.

#### Type Parameters

| Type Parameter |
| ------ |
| `T` |

#### Parameters

| Parameter | Type |
| ------ | ------ |
| `fn` | (`tx`: [`SqlExecutor`](SqlExecutor.md)) => `Promise`\<`T`\> |

#### Returns

`Promise`\<`T`\>
