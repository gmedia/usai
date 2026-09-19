[@sakaladev/usai](../../README.md) / [index](../README.md) / SqlExecutor

# Interface: SqlExecutor

The statements available on a connection. Rows are plain objects keyed
by column name; values arrive as JSON (uuid, timestamptz and numeric
as strings, integers and floats as numbers, json/jsonb as values).

## Extended by

- [`PostgresHandle`](PostgresHandle.md)

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
