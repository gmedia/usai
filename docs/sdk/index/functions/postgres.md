[@sakaladev/usai](../../README.md) / [index](../README.md) / postgres

# Function: postgres()

```ts
function postgres<N extends string>(name: N, options?: PostgresOptions): PostgresDeclaration<N>;
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

## Type Parameters

| Type Parameter |
| ------ |
| `N` *extends* `string` |

## Parameters

| Parameter | Type | Description |
| ------ | ------ | ------ |
| `name` | `N` | Unique within the application; the handle is `ctx.resources[name]`, typed when the workload lists this declaration under `resources`. |
| `options` | [`PostgresOptions`](../interfaces/PostgresOptions.md) | - |

## Returns

[`PostgresDeclaration`](../interfaces/PostgresDeclaration.md)\<`N`\>

## Example

```ts
export const db = postgres("db");                          // reads DATABASE_URL; `ctx.resources.db`
export const getUser = http.get("/users/:id", { params: Id, resources: [db] }, async (ctx) => {
  const user = await ctx.resources.db.one<User>("select * from users where id = $1", [ctx.params.id]);
  if (!user) throw errors.notFound();
  return user;
});
```
