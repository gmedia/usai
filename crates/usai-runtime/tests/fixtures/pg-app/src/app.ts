// PostgreSQL fixture for the D6 acceptance tests (contract C5).
import {
  defineApp,
  http,
  task,
  command,
  postgres,
  errors,
  env,
  queue,
  cache,
  bytes,
} from "@sakaladev/usai";
import { z } from "zod";

const db = postgres("main", { pool: { max: 4 } });
type Db = import("@sakaladev/usai").PostgresHandle;
const d = (ctx: { resources: Record<string, unknown> }) => ctx.resources["main"] as Db;

const User = z.object({ id: z.number().int(), name: z.string(), email: z.string() });

export const setup = command("setup", { resources: [db] }, async (ctx) => {
  await d(ctx).execute(
    `create table if not exists users (id serial primary key, name text not null, email text not null unique)`,
  );
  await d(ctx).execute(
    `create table if not exists blobs (id serial primary key, data bytea not null)`,
  );
  await d(ctx).execute(`truncate users restart identity`);
  await d(ctx).execute(`insert into users (name, email) values ($1, $2), ($3, $4)`, [
    "Ayu",
    "ayu@x.io",
    "Budi",
    "budi@x.io",
  ]);
  return { ok: true };
});

export const getUser = http.get(
  "/users/:id",
  {
    params: z.object({ id: z.coerce.number().int().min(1) }),
    response: { 200: User },
    resources: [db],
  },
  async (ctx) => {
    const row = await d(ctx).one<{ id: number; name: string; email: string }>(
      `select id, name, email from users where id = $1`,
      [ctx.params.id],
    );
    if (!row) throw errors.notFound("user not found");
    return row;
  },
);

// Bytes both ways: a Uint8Array parameter binds to bytea, a bytea column
// arrives as base64 and `bytes.fromBase64` gives the Uint8Array back.
export const putBlob = http.get(
  "/blobs/put",
  { query: z.object({ hex: z.string().regex(/^([0-9a-f]{2})*$/) }), resources: [db] },
  async (ctx) => {
    const data = new Uint8Array(ctx.query.hex.match(/../g)?.map((h) => parseInt(h, 16)) ?? []);
    const row = await d(ctx).one<{ id: number; length: number }>(
      `insert into blobs (data) values ($1) returning id, length(data) as length`,
      [data],
    );
    return row;
  },
);
export const getBlob = http.get(
  "/blobs/:id",
  { params: z.object({ id: z.coerce.number().int() }), resources: [db] },
  async (ctx) => {
    const row = await d(ctx).one<{ data: string }>(`select data from blobs where id = $1`, [
      ctx.params.id,
    ]);
    if (!row) throw errors.notFound("no blob");
    const data = bytes.fromBase64(row.data);
    return {
      hex: Array.from(data, (b) => b.toString(16).padStart(2, "0")).join(""),
      base64: row.data,
    };
  },
);

export const listUsers = http.get("/users", { resources: [db] }, async (ctx) =>
  d(ctx).query(`select id, name from users order by id`),
);

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

export const tls = http.get("/tls", { resources: [db] }, async (ctx) =>
  d(ctx).one<{ ssl: boolean }>(`select ssl from pg_stat_ssl where pid = pg_backend_pid()`),
);

export const types = task("types", { resources: [db] }, async (ctx) =>
  d(ctx).one(
    `select $1::int8 as big, $2::float8 as f, $3::bool as b, $4::uuid as u, $5::jsonb as j, $6::timestamptz as t, $7::text[] as arr, 12.50::numeric as n, null::text as nothing`,
    [
      9007199254740991,
      1.5,
      true,
      "6f1a2b3c-4d5e-4f60-8a71-92b3c4d5e6f7",
      { a: [1, 2] },
      "2026-09-17T10:00:00Z",
      ["x", "y"],
    ],
  ),
);

// Types without a binary encoder take their text form: interval, inet,
// and a value read back with ::text (PostgreSQL's own timestamp format).
export const textTypes = task("text-types", { resources: [db] }, async (ctx) =>
  d(ctx).one(
    `select ($1::interval)::text as i, ($2::inet)::text as ip, $3::timestamptz as t, ($4::numeric)::text as n`,
    ["30 days", "10.0.0.1", "2026-09-18 19:48:41.507406+07", "12.50"],
  ),
);

export const badParams = task("bad-params", { resources: [db] }, async (ctx) => {
  try {
    await d(ctx).one(`select $1::int`, ["not a number"]);
    return "unexpected";
  } catch (e) {
    return (e as { usai: { code: string } }).usai.code;
  }
});

export const leak = http.get("/leak", { resources: [db] }, async (ctx) => {
  // A session setting must not survive into another world's lease if the
  // connection is reused: reads back what this world sees.
  const before = await d(ctx).one<{ v: string }>(
    `select current_setting('usai.marker', true) as v`,
  );
  await d(ctx).execute(`select set_config('usai.marker', 'set-by-world', false)`);
  return { before: before?.v ?? null };
});

// ---- transactions ----------------------------------------------------------
export const txCommit = http.post("/tx/commit", { resources: [db] }, async (ctx) =>
  d(ctx).transaction(async (tx) => {
    await tx.execute(`insert into users (name, email) values ($1, $2)`, ["Citra", "citra@x.io"]);
    // Visible inside the transaction, on the same connection.
    const inside = await tx.one<{ n: number }>(
      `select count(*)::int as n from users where email = $1`,
      ["citra@x.io"],
    );
    return { inside: inside?.n };
  }),
);

export const txRollback = http.post("/tx/rollback", { resources: [db] }, async (ctx) => {
  try {
    await d(ctx).transaction(async (tx) => {
      await tx.execute(`insert into users (name, email) values ($1, $2)`, ["Dewi", "dewi@x.io"]);
      throw errors.conflict("changed my mind");
    });
    return { unexpected: true };
  } catch (e) {
    const n = await d(ctx).one<{ n: number }>(
      `select count(*)::int as n from users where email = $1`,
      ["dewi@x.io"],
    );
    return { code: (e as { usai: { code: string } }).usai.code, after: n?.n };
  }
});

export const txAbandon = http.post("/tx/abandon", { resources: [db] }, async (ctx) => {
  // Starts a transaction and returns without ending it: a lifecycle error
  // the runtime must diagnose, then roll back on the world's behalf.
  const handle = d(ctx);
  void handle.transaction(async (tx) => {
    await tx.execute(`insert into users (name, email) values ($1, $2)`, ["Eka", "eka@x.io"]);
    await new Promise(() => {});
  });
  await ctx.sleep(50);
  return { started: true };
});

export const txClosed = http.post("/tx/closed", { resources: [db] }, async (ctx) => {
  let leaked: import("@sakaladev/usai").SqlExecutor | null = null;
  await d(ctx).transaction(async (tx) => {
    leaked = tx;
  });
  try {
    await leaked!.query(`select 1`);
    return { unexpected: true };
  } catch (e) {
    return { code: (e as { usai: { code: string } }).usai.code };
  }
});

export const count = http.get("/count/:email", { resources: [db] }, async (ctx) =>
  d(ctx).one(`select count(*)::int as n from users where email = $1`, [ctx.params["email"]!]),
);

// ---- D10: queue -----------------------------------------------------------
const seen = cache.local("seen");
type Seen = { increment(k: string): Promise<number>; get(k: string): Promise<number | null> };

export const orders = queue.consume(
  "orders",
  {
    message: z.object({
      orderId: z.string(),
      fail: z.number().int().optional(),
      sleepMs: z.number().int().optional(),
    }),
    concurrency: 2,
    retry: { maxAttempts: 3, backoff: "fixed", baseMs: 100 },
    resources: [seen, db],
  },
  async (ctx) => {
    globalThis.__mutable = ((globalThis.__mutable as number | undefined) ?? 0) + 1;
    if (ctx.message.sleepMs) await ctx.sleep(ctx.message.sleepMs);
    const n = await (ctx.resources["seen"] as Seen).increment(`orders:${ctx.message.orderId}`);
    if (ctx.message.fail !== undefined && ctx.attempt <= ctx.message.fail)
      throw errors.internal(`attempt ${ctx.attempt} failed on purpose`);
    return {
      orderId: ctx.message.orderId,
      attempt: ctx.attempt,
      worldCounter: globalThis.__mutable,
      seen: n,
    };
  },
);

export const publish = http.post(
  "/orders",
  {
    body: z.object({
      orderId: z.string(),
      fail: z.number().int().optional(),
      sleepMs: z.number().int().optional(),
    }),
  },
  async (ctx) => ctx.queue.publish("orders", ctx.body),
);
export const seenCount = http.get("/seen/:key", { resources: [seen] }, async (ctx) => ({
  n: await (ctx.resources["seen"] as Seen).get(ctx.params["key"]!),
}));

declare global {
  // eslint-disable-next-line no-var
  var __mutable: unknown;
}

export default defineApp({
  name: "pg-fixture",
  workloads: [
    setup,
    getUser,
    listUsers,
    putBlob,
    getBlob,
    slow,
    fail,
    tls,
    types,
    textTypes,
    badParams,
    leak,
    txCommit,
    txRollback,
    txAbandon,
    txClosed,
    count,
    orders,
    publish,
    seenCount,
  ],
  resources: [db, seen],
  env: env({ DATABASE_URL: env.url() }),
});
