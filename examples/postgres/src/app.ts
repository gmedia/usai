// A Usai application over PostgreSQL.
//
//   export DATABASE_URL=postgres://user:pass@localhost:5432/app
//   cargo run -p usai-cli -- --root examples/postgres app setup
//   cargo run -p usai-cli -- dev --root examples/postgres
//   curl -X POST localhost:3000/users -H 'content-type: application/json' -d '{"name":"Ayu","email":"ayu@x.io"}'
//   curl localhost:3000/users/1
//
// The pool lives for the runtime; each request leases one connection for
// each operation and the runtime decides reuse from terminal proof.
import { defineApp, http, command, postgres, errors, env, type PostgresHandle } from "@sakaladev/usai";
import { z } from "zod";

const db = postgres("main");

const User = z.object({ id: z.number().int(), name: z.string(), email: z.string().email() });
const NewUser = z.object({ name: z.string().min(1), email: z.string().email() });
const Id = z.object({ id: z.coerce.number().int().min(1) });

export const setup = command("setup", { resources: [db] }, async (ctx) => {
  const sql = ctx.resources["main"] as PostgresHandle;
  await sql.execute(`create table if not exists users (id serial primary key, name text not null, email text not null unique)`);
  return { ok: true };
});

export const createUser = http.post("/users", { body: NewUser, response: { 201: User }, resources: [db] }, async (ctx) => {
  const sql = ctx.resources["main"] as PostgresHandle;
  try {
    const row = await sql.one<z.infer<typeof User>>(`insert into users (name, email) values ($1, $2) returning id, name, email`, [ctx.body.name, ctx.body.email]);
    return http.created(row!);
  } catch (e) {
    if ((e as { usai?: { code: string } }).usai?.code === "sql_23505") throw errors.conflict("email already registered");
    throw e;
  }
});

export const getUser = http.get("/users/:id", { params: Id, response: { 200: User }, resources: [db] }, async (ctx) => {
  const sql = ctx.resources["main"] as PostgresHandle;
  const row = await sql.one<z.infer<typeof User>>(`select id, name, email from users where id = $1`, [ctx.params.id]);
  if (!row) throw errors.notFound("user not found");
  return row;
});

export const listUsers = http.get("/users", { response: { 200: z.array(User) }, resources: [db] }, async (ctx) => {
  const sql = ctx.resources["main"] as PostgresHandle;
  return sql.query<z.infer<typeof User>>(`select id, name, email from users order by id`);
});

export default defineApp({
  name: "postgres-example",
  workloads: [setup, createUser, getUser, listUsers],
  resources: [db],
  env: env({ DATABASE_URL: env.url() }),
});
