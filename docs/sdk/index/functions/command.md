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
function command(
   name: string, 
   options: WorkloadPolicies & {
  resources?: ResourceDeclaration[];
}, 
   handler: (ctx: CommandContext) => unknown
): Workload;
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
| `options` | [`WorkloadPolicies`](../interfaces/WorkloadPolicies.md) & \{ `resources?`: [`ResourceDeclaration`](../interfaces/ResourceDeclaration.md)[]; \} |
| `handler` | (`ctx`: [`CommandContext`](../interfaces/CommandContext.md)) => `unknown` |

### Returns

[`Workload`](../interfaces/Workload.md)

### Example

```ts
export const stats = command("invoices:stats", { resources: [db] }, async (ctx) =>
  ctx.resources.db.one("select count(*)::int as invoices from invoices"),
);
```
