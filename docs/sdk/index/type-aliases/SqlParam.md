[@sakaladev/usai](../../README.md) / [index](../README.md) / SqlParam

# Type Alias: SqlParam

```ts
type SqlParam = 
  | string
  | number
  | boolean
  | null
  | Record<string, unknown>
  | unknown[];
```

A statement parameter (`$1`, `$2`, …): scalars bind to their SQL type,
objects and arrays bind as JSON (`jsonb`); cast in SQL when a column
needs something else (`$1::uuid[]`).
