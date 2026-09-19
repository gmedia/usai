// Bun + Hono comparator: Bun's built-in SQL client (`Bun.sql`, PostgreSQL),
// Hono's router, Zod on input and output — the same work as the Usai app.
// PORT, DATABASE_URL, POOL_MAX.
import { Hono } from "hono";
import { SQL } from "bun";
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

const sql = new SQL({ url: process.env.DATABASE_URL, max: Number(process.env.POOL_MAX ?? 64) });
const app = new Hono();
let counter = 0;

class Reply extends Error {
  constructor(
    public status: number,
    public body: unknown,
  ) {
    super("reply");
  }
}
const validate = <T>(
  schema: { safeParse(v: unknown): { success: boolean; data?: T; error?: unknown } },
  slot: string,
  value: unknown,
): T => {
  const r = schema.safeParse(value);
  if (!r.success)
    throw new Reply(
      400,
      err("validation_failed", `${slot} failed validation`, {
        slot,
        issues: issues(r.error as never),
      }),
    );
  return r.data as T;
};
const out = (
  c: { json(v: unknown, s?: number): Response },
  status: number,
  schema: { safeParse(v: unknown): { success: boolean; data?: unknown } },
  value: unknown,
) => {
  const r = schema.safeParse(value);
  if (!r.success)
    return c.json(err("response_contract_violation", "response does not match its contract"), 500);
  return c.json(r.data, status as never);
};
app.onError((e, c) =>
  e instanceof Reply
    ? c.json(e.body as never, e.status as never)
    : c.json(err("internal", "internal error"), 500),
);
app.notFound((c) =>
  c.json(
    err("route_not_found", `no route matches ${c.req.method} ${new URL(c.req.url).pathname}`),
    404,
  ),
);

app.get("/health", (c) => c.json({ ok: true }));
app.get("/hello/:name", (c) => {
  const p = validate<{ name: string }>(Name, "params", c.req.param());
  return out(c, 200, Hello, { hello: p.name });
});
app.post("/orders/quote", async (c) => {
  const b = validate<Parameters<typeof quote>[0]>(
    Quote,
    "body",
    await c.req.json().catch(() => null),
  );
  return out(c, 201, Quoted, quote(b));
});
app.get("/users/:id", async (c) => {
  const p = validate<{ id: number }>(Id, "params", c.req.param());
  const rows = await sql`select id, name, email from users where id = ${p.id}`;
  if (!rows[0]) return c.json(err("not_found", "user_not_found"), 404);
  return out(c, 200, User, rows[0]);
});
app.post("/users", async (c) => {
  const b = validate<{ name: string; email: string }>(
    NewUser,
    "body",
    await c.req.json().catch(() => null),
  );
  try {
    const rows =
      await sql`insert into users (name, email) values (${b.name}, ${b.email}) returning id, name, email`;
    return out(c, 201, User, rows[0]);
  } catch (e) {
    if ((e as { code?: string }).code === "23505" || String(e).includes("duplicate key"))
      return c.json(err("conflict", "email_taken"), 409);
    throw e;
  }
});
app.post("/orders/:id/pay", async (c) => {
  const p = validate<{ id: number }>(Id, "params", c.req.param());
  let reply: Response | null = null;
  const result = await sql.begin(async (tx) => {
    const rows =
      await tx`select id, total_cents as "totalCents", paid from orders where id = ${p.id} for update`;
    const order = rows[0];
    if (!order) {
      reply = c.json(err("not_found", "order_not_found"), 404);
      return null;
    }
    if (order.paid) {
      reply = c.json(err("conflict", "already_paid"), 409);
      return null;
    }
    await tx`update orders set paid = true where id = ${order.id}`;
    const pay =
      await tx`insert into payments (order_id, amount_cents) values (${order.id}, ${order.totalCents}) returning id`;
    return { orderId: order.id, paymentId: pay[0].id, amountCents: order.totalCents, paid: true };
  });
  if (reply) return reply;
  return out(c, 200, Paid, result);
});
app.get("/me", async (c) => {
  const key = c.req.header("x-api-key");
  if (!key) return c.json(err("unauthorized", "missing x-api-key header"), 401);
  const k = await sql`select user_id from api_keys where key = ${key}`;
  if (!k[0]) return c.json(err("unauthorized", "unknown_key"), 401);
  const rows = await sql`select id, name, email from users where id = ${k[0].user_id}`;
  if (!rows[0]) return c.json(err("not_found", "user_not_found"), 404);
  return out(c, 200, User, rows[0]);
});
app.get("/counter", (c) => out(c, 200, Counter, { count: ++counter }));
app.get("/slow", async (c) => {
  const q = validate<{ ms: number }>(SlowQuery, "query", c.req.query());
  await Bun.sleep(q.ms);
  return out(c, 200, Slept, { slept: q.ms });
});

export default { port: Number(process.env.PORT ?? 3002), hostname: "127.0.0.1", fetch: app.fetch };
