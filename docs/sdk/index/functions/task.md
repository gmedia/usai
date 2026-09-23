[@sakaladev/usai](../../README.md) / [index](../README.md) / task

# Function: task()

```ts
function task<I extends AnySchema | undefined = undefined, R extends ResourceDeclaration<string, unknown>[] = ResourceDeclaration<string, unknown>[], Out = unknown>(
   name: string, 
   options: TaskOptions<I, R>, 
   handler: (ctx: TaskContext<I extends AnySchema ? Output<I> : unknown, R>) => Out
): TypedWorkload<Awaited<Out>>;
```

Declare a task: a named unit of finite work that other workloads invoke
or dispatch, and that `usai task run <name>` runs by hand.

A task always runs in a **fresh world of its own**, never inside the
caller's. Who owns that world is the caller's choice at the call site:
`ctx.tasks.invoke(task, input)` is **owned** — the caller waits for the
result and the child is cancelled with the caller; `ctx.tasks.dispatch(task,
input)` is **ownership transfer** — the task runtime owns the child, the
caller's world may end, and the returned `{ id }` is the only handle.
There is no third option: a finite world that ends with live async work
is a runtime error. Dispatch is in-process and not durable across a
restart; for durable hand-off publish to a queue.

## Type Parameters

| Type Parameter | Default type |
| ------ | ------ |
| `I` *extends* [`AnySchema`](../type-aliases/AnySchema.md) \| `undefined` | `undefined` |
| `R` *extends* [`ResourceDeclaration`](../interfaces/ResourceDeclaration.md)\<`string`, `unknown`\>[] | [`ResourceDeclaration`](../interfaces/ResourceDeclaration.md)\<`string`, `unknown`\>[] |
| `Out` | `unknown` |

## Parameters

| Parameter | Type | Description |
| ------ | ------ | ------ |
| `name` | `string` | Unique within the application; the id is `task:<name>`. Any text; a colon is fine (`invoices:remind`). |
| `options` | [`TaskOptions`](../interfaces/TaskOptions.md)\<`I`, `R`\> | Input schema, errors, resources, `timeout`, `concurrency`. |
| `handler` | (`ctx`: [`TaskContext`](../interfaces/TaskContext.md)\<`I` *extends* [`AnySchema`](../type-aliases/AnySchema.md) ? [`Output`](../type-aliases/Output.md)\<`I`\> : `unknown`, `R`\>) => `Out` | Runs in the task's world. Its return value is the `invoke` result; after a `dispatch` nobody receives it — the outcome shows only in the runtime's log and the task counters of `/_usai/status`, so a dispatched task records what matters in a resource. |

## Returns

[`TypedWorkload`](../interfaces/TypedWorkload.md)\<`Awaited`\<`Out`\>\>

## Example

```ts
export const sendReceipt = task("send-receipt", { input: Receipt, resources: [db, mailer] }, async (ctx) => {
  const order = await ctx.resources.db.one("select … where id = $1", [ctx.input.orderId]);
  await ctx.resources.mailer.fetch("/send", { json: order });
});
// From an endpoint: hand it off, then answer.
export const pay = dispatches(
  http.post("/orders/:id/pay", { params: Id, resources: [db] }, async (ctx) => {
    await ctx.resources.db.execute("update orders set paid = true where id = $1", [ctx.params.id]);
    await ctx.tasks.dispatch(sendReceipt, { orderId: ctx.params.id });
    return http.accepted({ ok: true });
  }),
  sendReceipt,
);
```
