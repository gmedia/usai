// PostgreSQL fixture for the D6 acceptance tests (contract C5).
import { defineApp, http, task, command, postgres, errors, env } from "usai";
import { z } from "zod";

const db = postgres("main", { pool: { max: 4 } });
type Db = import("usai").PostgresHandle;
const d = (ctx: { resources: Record<string, unknown> }) => ctx.resources["main"] as Db;

const User = z.object({ id: z.number().int(), name: z.string(), email: z.string() });

export const setup = command("setup", { resources: [db] }, async (ctx) => {
  await d(ctx).execute(`create table if not exists users (id serial primary key, name text not null, email text not null unique)`);
  await d(ctx).execute(`truncate users restart identity`);
  await d(ctx).execute(`insert into users (name, email) values ($1, $2), ($3, $4)`, ["Ayu", "ayu@x.io", "Budi", "budi@x.io"]);
  return { ok: true };
});

export const getUser = http.get("/users/:id", { params: z.object({ id: z.coerce.number().int().min(1) }), response: { 200: User }, resources: [db] }, async (ctx) => {
  const row = await d(ctx).one<{ id: number; name: string; email: string }>(`select id, name, email from users where id = $1`, [ctx.params.id]);
  if (!row) throw errors.notFound("user not found");
  return row;
});

export const listUsers = http.get("/users", { resources: [db] }, async (ctx) => d(ctx).query(`select id, name from users order by id`));

export const slow = http.get("/slow", { timeout: "300ms", resources: [db] }, async (ctx) => {
  await d(ctx).one(`select pg_sleep(30)`);
  return { unreachable: true };
});

export const fail = http.get("/fail", { resources: [db] }, async (ctx) => {
  try {
    await d(ctx).one(`select 1 / $1::int`, [0]);
    return { unexpected: true };
  } catch (e) {
    return { code: (e as { usai: { code: string } }).usai.code };
  }
});

export const types = task("types", { resources: [db] }, async (ctx) =>
  d(ctx).one(
    `select $1::int8 as big, $2::float8 as f, $3::bool as b, $4::uuid as u, $5::jsonb as j, $6::timestamptz as t, $7::text[] as arr, 12.50::numeric as n, null::text as nothing`,
    [9007199254740991, 1.5, true, "6f1a2b3c-4d5e-4f60-8a71-92b3c4d5e6f7", { a: [1, 2] }, "2026-09-17T10:00:00Z", ["x", "y"]],
  ),
);

export const badParams = task("bad-params", { resources: [db] }, async (ctx) => {
  try { await d(ctx).one(`select $1::int`, ["not a number"]); return "unexpected"; } catch (e) { return (e as { usai: { code: string } }).usai.code; }
});

export const leak = http.get("/leak", { resources: [db] }, async (ctx) => {
  // A session setting must not survive into another world's lease if the
  // connection is reused: reads back what this world sees.
  const before = await d(ctx).one<{ v: string }>(`select current_setting('usai.marker', true) as v`);
  await d(ctx).execute(`select set_config('usai.marker', 'set-by-world', false)`);
  return { before: before?.v ?? null };
});

export default defineApp({
  name: "pg-fixture",
  workloads: [setup, getUser, listUsers, slow, fail, types, badParams, leak],
  resources: [db],
  env: env({ DATABASE_URL: env.url() }),
});
