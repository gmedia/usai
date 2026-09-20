// The queue throughput campaign (scripts/qualification/queue/run.sh): what
// the PostgreSQL-backed queue does per second, per consumer concurrency, on
// one and on two instances. The consumer does the smallest real work — one
// INSERT recording the message — so the number is the queue's, not the
// handler's. run.sh rewrites the concurrency below per cell before building
// (a world has no process.env; the number is part of the definition).
import { command, defineApp, env, http, postgres, queue } from "@sakaladev/usai";
import { z } from "zod";

export const db = postgres("db", { pool: { max: 16 } });

const concurrency = 8; // QUEUE_CONCURRENCY

const Message = z.object({ n: z.number().int(), batch: z.string() });

export const work = queue.consume(
  "bench.work",
  { message: Message, concurrency, database: db, resources: [db] },
  async (ctx) => {
    await ctx.resources.db.execute(
      "insert into processed (id, consumer) values ($1, $2) on conflict do nothing",
      [ctx.message.n, ctx.id],
    );
    return { n: ctx.message.n };
  },
);

// `usai app publish -- <count> <batch>`: publishes count messages from one
// world (each publish is an owned operation: one INSERT into usai_queue).
export const publish = command("publish", { resources: [db] }, async (ctx) => {
  const count = Number(ctx.args[0] ?? 1000);
  const batch = String(ctx.args[1] ?? Date.now());
  const started = Date.now();
  for (let n = 0; n < count; n++) await ctx.queue.publish("bench.work", { n, batch });
  return { published: count, ms: Date.now() - started };
});

// One message per request: what a producer route costs.
export const enqueue = http.post(
  "/enqueue",
  {
    body: z.object({ n: z.number().int(), batch: z.string() }),
    response: { 202: z.object({ ok: z.boolean() }) },
  },
  async (ctx) => {
    await ctx.queue.publish("bench.work", ctx.body);
    return http.accepted({ ok: true });
  },
);

export const health = http.get(
  "/health",
  { response: { 200: z.object({ ok: z.boolean() }) } },
  async () => ({ ok: true }),
);

export default defineApp({
  name: "queue-bench",
  workloads: [work, publish, enqueue, health],
  resources: [db],
  env: env({ DATABASE_URL: env.url() }),
});
