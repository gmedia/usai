// Node + Fastify comparator: the research denominator's stack (Fastify, pg
// pool), doing exactly the Usai app's work — Zod on input and output, the
// same SQL, the same error envelope. PORT, DATABASE_URL, POOL_MAX.
import Fastify from "fastify";
import pg from "pg";
import {
  Name,
  Hello,
  Quote,
  Quoted,
  Id,
  User,
  NewUser,
  Paid,
  Counter,
  SlowQuery,
  Slept,
  quote,
  err,
  issues,
} from "../shared-schemas.mjs";

const pool = new pg.Pool({
  connectionString: process.env.DATABASE_URL,
  max: Number(process.env.POOL_MAX ?? 64),
});
const app = Fastify({ logger: false });
let counter = 0;

const validate = (schema, slot, value) => {
  const r = schema.safeParse(value);
  if (!r.success) {
    const e = new Error(`${slot} failed validation`);
    e.reply = [400, err("validation_failed", e.message, { slot, issues: issues(r.error) })];
    throw e;
  }
  return r.data;
};
const out = (reply, status, schema, value) => {
  const r = schema.safeParse(value);
  if (!r.success)
    return reply
      .code(500)
      .send(err("response_contract_violation", "response does not match its contract"));
  return reply.code(status).send(r.data);
};
app.setErrorHandler((e, _req, reply) => {
  if (e.reply) return reply.code(e.reply[0]).send(e.reply[1]);
  if (e.validation) return reply.code(400).send(err("validation_failed", e.message));
  return reply.code(500).send(err("internal", "internal error"));
});
app.setNotFoundHandler((req, reply) =>
  reply.code(404).send(err("route_not_found", `no route matches ${req.method} ${req.url}`)),
);

app.get("/health", async (_req, reply) => reply.send({ ok: true }));
app.get("/hello/:name", async (req, reply) => {
  const p = validate(Name, "params", req.params);
  return out(reply, 200, Hello, { hello: p.name });
});
app.post("/orders/quote", async (req, reply) => {
  const b = validate(Quote, "body", req.body);
  return out(reply, 201, Quoted, quote(b));
});
app.get("/users/:id", async (req, reply) => {
  const p = validate(Id, "params", req.params);
  const { rows } = await pool.query("select id, name, email from users where id = $1", [p.id]);
  if (!rows[0]) return reply.code(404).send(err("not_found", "user_not_found"));
  return out(reply, 200, User, rows[0]);
});
app.post("/users", async (req, reply) => {
  const b = validate(NewUser, "body", req.body);
  try {
    const { rows } = await pool.query(
      "insert into users (name, email) values ($1, $2) returning id, name, email",
      [b.name, b.email],
    );
    return out(reply, 201, User, rows[0]);
  } catch (e) {
    if (e.code === "23505") return reply.code(409).send(err("conflict", "email_taken"));
    throw e;
  }
});
app.post("/orders/:id/pay", async (req, reply) => {
  const p = validate(Id, "params", req.params);
  const client = await pool.connect();
  try {
    await client.query("begin");
    const { rows } = await client.query(
      'select id, total_cents as "totalCents", paid from orders where id = $1 for update',
      [p.id],
    );
    const order = rows[0];
    if (!order) {
      await client.query("rollback");
      return reply.code(404).send(err("not_found", "order_not_found"));
    }
    if (order.paid) {
      await client.query("rollback");
      return reply.code(409).send(err("conflict", "already_paid"));
    }
    await client.query("update orders set paid = true where id = $1", [order.id]);
    const pay = await client.query(
      "insert into payments (order_id, amount_cents) values ($1, $2) returning id",
      [order.id, order.totalCents],
    );
    await client.query("commit");
    return out(reply, 200, Paid, {
      orderId: order.id,
      paymentId: pay.rows[0].id,
      amountCents: order.totalCents,
      paid: true,
    });
  } catch (e) {
    await client.query("rollback").catch(() => {});
    throw e;
  } finally {
    client.release();
  }
});
app.get("/me", async (req, reply) => {
  const key = req.headers["x-api-key"];
  if (!key) return reply.code(401).send(err("unauthorized", "missing x-api-key header"));
  const k = await pool.query("select user_id from api_keys where key = $1", [key]);
  if (!k.rows[0]) return reply.code(401).send(err("unauthorized", "unknown_key"));
  const { rows } = await pool.query("select id, name, email from users where id = $1", [
    k.rows[0].user_id,
  ]);
  if (!rows[0]) return reply.code(404).send(err("not_found", "user_not_found"));
  return out(reply, 200, User, rows[0]);
});
app.get("/counter", async (_req, reply) => out(reply, 200, Counter, { count: ++counter }));
app.get("/slow", async (req, reply) => {
  const q = validate(SlowQuery, "query", req.query);
  await new Promise((r) => setTimeout(r, q.ms));
  return out(reply, 200, Slept, { slept: q.ms });
});

await app.listen({ port: Number(process.env.PORT ?? 3001), host: "127.0.0.1" });
