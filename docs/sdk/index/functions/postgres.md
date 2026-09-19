[@sakaladev/usai](../../README.md) / [index](../README.md) / postgres

# Function: postgres()

```ts
function postgres(name: string, options?: PostgresOptions): PostgresDeclaration;
```

Declare a PostgreSQL resource. The runtime owns the pool for its whole
lifetime; a workload that lists the resource gets a [PostgresHandle](../interfaces/PostgresHandle.md)
at `ctx.resources.<name>`, and every statement leases one connection for
exactly that operation. The connection returns to the pool only after a
**terminal outcome** (the result or the error arrived); a world that
dies mid-statement proves nothing about the connection, so it is
quarantined, then removed and replaced, never reused. Connection loss and
pool exhaustion surface to HTTP callers as 503, not 500.

Migrations are SQL files matched by the module's `migrations` globs,
applied by `usai db migrate` (never at startup). The same resource can
be declared by several modules with the same configuration.

## Parameters

| Parameter | Type | Description |
| ------ | ------ | ------ |
| `name` | `string` | Unique within the application; `ctx.resources[name]`. |
| `options` | [`PostgresOptions`](../interfaces/PostgresOptions.md) | - |

## Returns

[`PostgresDeclaration`](../interfaces/PostgresDeclaration.md)

## Example

```ts
export const db = postgres("main");                       // reads DATABASE_URL
export const getUser = http.get("/users/:id", { params: Id, resources: [db] }, async (ctx) => {
  const user = await ctx.resources.db.one<User>("select * from users where id = $1", [ctx.params.id]);
  if (!user) throw errors.notFound();
  return user;
});
```
