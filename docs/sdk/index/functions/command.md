[@sakaladev/usai](../../README.md) / [index](../README.md) / command

# Function: command()

## Call Signature

```ts
function command(name: string, handler: (ctx: CommandContext) => unknown): Workload;
```

Declare a command: finite work run on demand from the command line
(`usai app <name> [args]`), in a fresh world with the declared resources.
The return value is printed as JSON; a thrown error exits non-zero.
Commands are for operators (a stats report, a one-off repair), not for
startup: nothing runs a command unless someone asks.

### Parameters

| Parameter | Type |
| ------ | ------ |
| `name` | `string` |
| `handler` | (`ctx`: [`CommandContext`](../interfaces/CommandContext.md)) => `unknown` |

### Returns

[`Workload`](../interfaces/Workload.md)

### Example

```ts
export const stats = command("invoices:stats", { resources: [db] }, async (ctx) =>
  ctx.resources.db.one("select count(*)::int as invoices from invoices"),
);
```

## Call Signature

```ts
function command<R extends ResourceDeclaration<string, unknown>[] = ResourceDeclaration<string, unknown>[]>(
   name: string, 
   options: CommandOptions<R>, 
   handler: (ctx: CommandContext<R>) => unknown
): Workload;
```

Declare a command: finite work run on demand from the command line
(`usai app <name> [args]`), in a fresh world with the declared resources.
The return value is printed as JSON; a thrown error exits non-zero.
Commands are for operators (a stats report, a one-off repair), not for
startup: nothing runs a command unless someone asks.

### Type Parameters

| Type Parameter | Default type |
| ------ | ------ |
| `R` *extends* [`ResourceDeclaration`](../interfaces/ResourceDeclaration.md)\<`string`, `unknown`\>[] | [`ResourceDeclaration`](../interfaces/ResourceDeclaration.md)\<`string`, `unknown`\>[] |

### Parameters

| Parameter | Type |
| ------ | ------ |
| `name` | `string` |
| `options` | [`CommandOptions`](../interfaces/CommandOptions.md)\<`R`\> |
| `handler` | (`ctx`: [`CommandContext`](../interfaces/CommandContext.md)\<`R`\>) => `unknown` |

### Returns

[`Workload`](../interfaces/Workload.md)

### Example

```ts
export const stats = command("invoices:stats", { resources: [db] }, async (ctx) =>
  ctx.resources.db.one("select count(*)::int as invoices from invoices"),
);
```
