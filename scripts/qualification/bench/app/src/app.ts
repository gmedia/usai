// The benchmark application: one route per workload class, the same
// semantics every comparator implements (validate input → do the work →
// validate and encode the output). Nothing here is tuned; it is written the
// way the GUIDE says to write an application.
//
//   A  GET  /hello/:name          runtime tax microscope
//   B  POST /orders/quote         contract-heavy: 12-field body, small CPU, response contract
//   C  GET  /users/:id            DB read (the research fixture's shape)
//   D  POST /users                DB write
//   E  POST /orders/:id/pay       transaction: lock, update, insert, in one transaction
//   F  GET  /me                   auth (API key looked up in PostgreSQL) + DB read
//   probes: GET /counter (state leak), GET /slow (cancellation), GET /health
import { auth, defineApp, env, errors, http, postgres } from "@sakaladev/usai";
import { z } from "zod";

export const db = postgres("db", { pool: { max: 64 } });

// ---- contracts (shared shapes; the comparators carry the same ones) ----
const Name = z.object({ name: z.string().min(1).max(40) });
const Hello = z.object({ hello: z.string() });

const Item = z.object({
  sku: z.string().min(1).max(32),
  quantity: z.number().int().min(1).max(1000),
  unitCents: z.number().int().min(0).max(10_000_000),
});
const Quote = z.object({
  customer: z.string().min(1).max(200),
  email: z.string().email(),
  currency: z.enum(["USD", "EUR", "IDR"]),
  country: z.string().length(2),
  couponCode: z.string().max(20).optional(),
  notes: z.string().max(500).optional(),
  priority: z.enum(["low", "normal", "high"]).default("normal"),
  gift: z.boolean().default(false),
  requestedDate: z.string().regex(/^\d{4}-\d{2}-\d{2}$/),
  reference: z.string().uuid(),
  tags: z.array(z.string().max(16)).max(10),
  items: z.array(Item).min(1).max(50),
});
const Quoted = z.object({
  reference: z.string(),
  currency: z.string(),
  subtotalCents: z.number().int(),
  taxCents: z.number().int(),
  totalCents: z.number().int(),
  lines: z.number().int(),
  priority: z.string(),
});

const Id = z.object({ id: z.coerce.number().int().min(1).max(2_147_483_647) });
const User = z.object({ id: z.number().int(), name: z.string(), email: z.string() });
const NewUser = z.object({ name: z.string().min(1).max(200), email: z.string().email() });
const Paid = z.object({
  orderId: z.number().int(),
  paymentId: z.number().int(),
  amountCents: z.number().int(),
  paid: z.boolean(),
});

// ---- A: hello ------------------------------------------------------------
export const hello = http.get(
  "/hello/:name",
  { summary: "Runtime tax microscope", params: Name, response: { 200: Hello } },
  async (ctx) => ({ hello: ctx.params.name }),
);

// ---- B: contract-heavy ---------------------------------------------------
export const quote = http.post(
  "/orders/quote",
  {
    summary: "Contract-heavy: validate 12 fields, compute, encode",
    body: Quote,
    response: { 201: Quoted },
  },
  async (ctx) => {
    const subtotal = ctx.body.items.reduce((sum, item) => sum + item.quantity * item.unitCents, 0);
    const rate = ctx.body.country === "ID" ? 11 : ctx.body.country === "DE" ? 19 : 0;
    const tax = Math.round((subtotal * rate) / 100);
    return http.created({
      reference: ctx.body.reference,
      currency: ctx.body.currency,
      subtotalCents: subtotal,
      taxCents: tax,
      totalCents: subtotal + tax,
      lines: ctx.body.items.length,
      priority: ctx.body.priority,
    });
  },
);

// ---- C: DB read ----------------------------------------------------------
export const getUser = http.get(
  "/users/:id",
  {
    summary: "DB read",
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

// ---- D: DB write ---------------------------------------------------------
export const createUser = http.post(
  "/users",
  {
    summary: "DB write",
    body: NewUser,
    response: { 201: User },
    errors: [{ code: "conflict", status: 409 }],
    resources: [db],
  },
  async (ctx) => {
    try {
      const row = await ctx.resources.db.one<z.infer<typeof User>>(
        "insert into users (name, email) values ($1, $2) returning id, name, email",
        [ctx.body.name, ctx.body.email],
      );
      return http.created(row!);
    } catch (e) {
      if ((e as { usai?: { code: string } }).usai?.code === "sql_23505")
        throw errors.conflict("email_taken");
      throw e;
    }
  },
);

// ---- E: transaction ------------------------------------------------------
export const pay = http.post(
  "/orders/:id/pay",
  {
    summary: "Transaction: lock the order, mark it paid, record the payment",
    params: Id,
    response: { 200: Paid },
    errors: [
      { code: "not_found", status: 404 },
      { code: "conflict", status: 409 },
    ],
    resources: [db],
  },
  async (ctx) => {
    return ctx.resources.db.transaction(async (tx) => {
      const order = await tx.one<{ id: number; totalCents: number; paid: boolean }>(
        'select id, total_cents as "totalCents", paid from orders where id = $1 for update',
        [ctx.params.id],
      );
      if (!order) throw errors.notFound("order_not_found");
      if (order.paid) throw errors.conflict("already_paid");
      await tx.execute("update orders set paid = true where id = $1", [order.id]);
      const payment = await tx.one<{ id: number }>(
        "insert into payments (order_id, amount_cents) values ($1, $2) returning id",
        [order.id, order.totalCents],
      );
      return {
        orderId: order.id,
        paymentId: payment!.id,
        amountCents: order.totalCents,
        paid: true,
      };
    });
  },
);

// ---- F: auth + DB --------------------------------------------------------
type Principal = { userId: number };
export const apiKey = auth.header<Principal>({
  name: "apiKey",
  header: "x-api-key",
  description: "A key from the api_keys table (key-1 … key-1000)",
  resolve: async (ctx, value) => {
    const row = await (
      ctx.resources.db as { one<T>(sql: string, params: unknown[]): Promise<T | null> }
    ).one<{ userId: number }>('select user_id as "userId" from api_keys where key = $1', [value]);
    if (!row) throw errors.unauthorized("unknown_key");
    return row;
  },
});
export const me = http.get(
  "/me",
  {
    summary: "Auth (API key in PostgreSQL) then a DB read",
    auth: apiKey,
    response: { 200: User },
    resources: [db],
  },
  async (ctx) => {
    const row = await ctx.resources.db.one<z.infer<typeof User>>(
      "select id, name, email from users where id = $1",
      [ctx.auth.userId],
    );
    if (!row) throw errors.notFound("user_not_found");
    return row;
  },
);

// ---- probes ---------------------------------------------------------------
// State leak: a module-level counter. Under Usai every request runs in a
// fresh world, so this always answers 1; a runtime that keeps the module
// alive answers the request number. Neither is "wrong" — it is the
// difference the suite is measuring.
let counter = 0;
export const count = http.get(
  "/counter",
  {
    summary: "Cross-request state probe",
    response: { 200: z.object({ count: z.number().int() }) },
  },
  async () => ({ count: ++counter }),
);
export const slow = http.get(
  "/slow",
  {
    summary: "Cancellation probe",
    query: z.object({ ms: z.coerce.number().int().min(0).max(30_000).default(1000) }),
    response: { 200: z.object({ slept: z.number().int() }) },
  },
  async (ctx) => {
    await ctx.sleep(ctx.query.ms);
    return { slept: ctx.query.ms };
  },
);
export const health = http.get(
  "/health",
  { summary: "Readiness for the runner", response: { 200: z.object({ ok: z.boolean() }) } },
  async () => ({ ok: true }),
);

export default defineApp({
  name: "bench",
  description: "The workload classes of the Usai benchmark suite.",
  workloads: [hello, quote, getUser, createUser, pay, me, count, slow, health],
  resources: [db],
  env: env({ DATABASE_URL: env.url() }),
});
