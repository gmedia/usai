import { defineModule, http, cron, command, dispatches, errors, type PostgresHandle } from "@sakaladev/usai";
import { z } from "zod";
import { db } from "../resources.ts";
import { record } from "../activity/module.ts";

const Todo = z.object({ id: z.number().int(), title: z.string(), done: z.boolean(), createdAt: z.string(), completedAt: z.string().nullable() });
const NewTodo = z.object({ title: z.string().min(1).max(200) });
const Id = z.object({ id: z.coerce.number().int().min(1) });
const ListQuery = z.object({ done: z.enum(["true", "false"]).optional(), limit: z.coerce.number().int().min(1).max(100).default(20) });
type TodoRow = z.infer<typeof Todo>;

const sql = (ctx: { resources: Record<string, unknown> }) => ctx.resources["main"] as PostgresHandle;
const columns = `id, title, done, created_at as "createdAt", completed_at as "completedAt"`;

export const list = http.get("/todos", { query: ListQuery, response: { 200: z.array(Todo) }, resources: [db] }, async (ctx) => {
  const where = ctx.query.done === undefined ? "" : `where done = ${ctx.query.done === "true"}`;
  return sql(ctx).query<TodoRow>(`select ${columns} from todos ${where} order by id limit $1`, [ctx.query.limit]);
});

export const get = http.get("/todos/:id", { params: Id, response: { 200: Todo }, resources: [db] }, async (ctx) => {
  const row = await sql(ctx).one<TodoRow>(`select ${columns} from todos where id = $1`, [ctx.params.id]);
  if (!row) throw errors.notFound(`todo ${ctx.params.id} does not exist`);
  return row;
});

// `dispatches(...)` records the hand-off for `usai graph` and the API docs.
export const create = dispatches(
  http.post("/todos", { body: NewTodo, response: { 201: Todo }, resources: [db] }, async (ctx) => {
    const row = await sql(ctx).one<TodoRow>(`insert into todos (title) values ($1) returning ${columns}`, [ctx.body.title]);
    // The request answers now; the activity row is written by a task in its
    // own world. Nothing detached is left behind in this world.
    await ctx.tasks.dispatch(record, { todoId: row!.id, event: "created" });
    return http.created(row!);
  }),
  record,
);

// `dispatches(...)` records the hand-off for `usai graph` and the API docs.
export const complete = dispatches(
  http.post("/todos/:id/complete", { params: Id, response: { 200: Todo }, resources: [db] }, async (ctx) => {
    const row = await sql(ctx).one<TodoRow>(`update todos set done = true, completed_at = now() where id = $1 and done = false returning ${columns}`, [ctx.params.id]);
    if (!row) throw errors.conflict(`todo ${ctx.params.id} is already done or does not exist`);
    await ctx.tasks.dispatch(record, { todoId: row.id, event: "completed" });
    return row;
  }),
  record,
);

// `dispatches(...)` records the hand-off for `usai graph` and the API docs.
export const remove = dispatches(
  http.delete("/todos/:id", { params: Id, resources: [db] }, async (ctx) => {
    const n = await sql(ctx).execute(`delete from todos where id = $1`, [ctx.params.id]);
    if (n === 0) throw errors.notFound(`todo ${ctx.params.id} does not exist`);
    await ctx.tasks.dispatch(record, { todoId: ctx.params.id, event: "deleted" });
    return http.noContent();
  }),
  record,
);

// Runs in a fresh world on schedule; `usai cron run purge-completed` or the
// test harness invokes it without waiting for the wall clock.
export const purge = cron("purge-completed", { schedule: "0 3 * * *", resources: [db] }, async (ctx) => {
  const n = await sql(ctx).execute(`delete from todos where done and completed_at < now() - interval '30 days'`);
  return { purged: n };
});

// `usai app stats` — a finite world, no server needed.
export const stats = command("stats", { resources: [db] }, async (ctx) => {
  const row = await sql(ctx).one<{ total: number; done: number }>(`select count(*)::int as total, count(*) filter (where done)::int as done from todos`);
  return row;
});

export const todos = defineModule({
  name: "todos",
  workloads: [list, get, create, complete, remove, purge, stats],
  resources: [db],
  migrations: "./src/todos/migrations/*.sql",
  seeders: "./src/todos/seeders/*.ts",
});
