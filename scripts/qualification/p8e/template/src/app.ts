// One tenant of the efficiency fleet (docs/measurements/BENCHMARKS.md,
// scoreboard 2). The bench app's shape — hello, one PostgreSQL read, one
// write — plus a task and a cron so the scheduler and the task queue exist,
// with `pool.max` 4: the supported-floor arithmetic is N × 4 connections.
import { cron, defineApp, env, errors, http, postgres, task } from "@sakaladev/usai";
import { z } from "zod";

export const db = postgres("db", { pool: { max: 4 } });

const Name = z.object({ name: z.string().min(1).max(40) });
const Id = z.object({ id: z.coerce.number().int().min(1).max(2_147_483_647) });
const User = z.object({ id: z.number().int(), name: z.string(), email: z.string() });
const NewUser = z.object({ name: z.string().min(1).max(200), email: z.string().email() });

export const hello = http.get(
  "/hello/:name",
  { params: Name, response: { 200: z.object({ hello: z.string() }) } },
  async (ctx) => ({ hello: ctx.params.name }),
);

export const getUser = http.get(
  "/users/:id",
  {
    params: Id,
    response: { 200: User },
    errors: [{ code: "not_found", status: 404 }],
    resources: [db],
  },
  async (ctx) => {
    const row = await ctx.resources.db.one<z.infer<typeof User>>(
      "select id, name, email from users where id = $1",
      [ctx.params.id],
    );
    if (!row) throw errors.notFound("user_not_found");
    return row;
  },
);

export const createUser = http.post(
  "/users",
  {
    body: NewUser,
    response: { 201: User },
    errors: [{ code: "conflict", status: 409 }],
    resources: [db],
  },
  async (ctx) => {
    const row = await ctx.resources.db.one<z.infer<typeof User>>(
      "insert into users (name, email) values ($1, $2) on conflict (email) do nothing returning id, name, email",
      [ctx.body.name, ctx.body.email],
    );
    if (!row) throw errors.conflict("email_taken");
    return http.created(row);
  },
);

export const health = http.get(
  "/health",
  { response: { 200: z.object({ ok: z.boolean() }) } },
  async () => ({ ok: true }),
);

// Present so the runtime holds a task queue and a cron scheduler like a
// real application; the cron never fires during a run.
export const reindex = task(
  "reindex",
  { input: z.object({ n: z.number().int() }) },
  async (ctx) => ({
    n: ctx.input.n,
  }),
);

export const nightly = cron("nightly", { schedule: "0 3 * * *", resources: [db] }, async (ctx) => {
  const row = await ctx.resources.db.one<{ n: number }>("select count(*)::int as n from users");
  return { users: row?.n ?? 0 };
});

export default defineApp({
  name: "p8e-tenant",
  workloads: [hello, getUser, createUser, health, reindex, nightly],
  resources: [db],
  env: env({ DATABASE_URL: env.url() }),
});
