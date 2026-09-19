[@sakaladev/usai](../../README.md) / [index](../README.md) / cron

# Function: cron()

```ts
function cron(
   name: string, 
   options: CronOptions, 
   handler: (ctx: CronContext) => unknown
): Workload;
```

Declare a scheduled job. Each due tick runs in a **fresh world**, finite,
bounded by `timeout`. The scheduler belongs to the revision: it starts
when the revision activates and stops when it drains, so two revisions
never tick the same job at once. A missed tick (the process was down)
is not replayed. `usai cron run <name>` runs one tick without the
clock, and `app.cron(name).run()` does the same in tests.

## Parameters

| Parameter | Type | Description |
| ------ | ------ | ------ |
| `name` | `string` | Unique within the application; the id is `cron:<name>`. |
| `options` | [`CronOptions`](../interfaces/CronOptions.md) | `schedule` (required), `overlap`, resources, `timeout`. |
| `handler` | (`ctx`: [`CronContext`](../interfaces/CronContext.md)) => `unknown` | Runs once per tick. |

## Returns

[`Workload`](../interfaces/Workload.md)

## Example

```ts
export const markOverdue = publishes(
  cron("mark-overdue", { schedule: "15 0 * * *", resources: [db] }, async (ctx) => {
    const rows = await ctx.resources.db.query("update invoices … returning id");
    for (const row of rows) await ctx.queue.publish("webhook.deliver", { event: "invoice.overdue", id: row.id });
  }),
  "webhook.deliver",
);
```
