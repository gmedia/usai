// HTTP fixture for the runtime's D2 acceptance tests. Every endpoint exists
// to prove one contract; see crates/usai-runtime/tests/http.rs.
import { defineApp, defineModule, http, auth, cache, errors, env, task } from "usai";
import { z } from "zod";

const Params = z.object({ id: z.string().uuid() });
const User = z.object({ id: z.string().uuid(), name: z.string(), email: z.string().email() });
const Query = z.object({ page: z.number().int().min(1).default(1), tags: z.array(z.string()).optional() });
const Body = z.object({ name: z.string().min(1), email: z.string().email() });

const hits = cache.local("hits");

const authenticated = auth.bearer({
  name: "token",
  resolve: async (_ctx, token) => {
    if (token !== "secret") throw errors.unauthorized("bad token");
    return { userId: "u1" };
  },
});

export const getUser = http.get("/users/:id", { params: Params, query: Query, response: { 200: User } }, async (ctx) => {
  if (ctx.params.id === "00000000-0000-0000-0000-000000000000") throw errors.notFound("user not found", { id: ctx.params.id });
  return { id: ctx.params.id, name: `user page ${ctx.query.page}`, email: "a@b.co" };
});

export const createUser = http.post("/users", { body: Body, response: { 201: User } }, async (ctx) =>
  http.created({ id: "6f1a2b3c-4d5e-4f60-8a71-92b3c4d5e6f7", ...ctx.body }),
);

export const counter = http.get("/counter", {}, async () => {
  globalThis.__mutable = ((globalThis.__mutable as number | undefined) ?? 0) + 1;
  return { counter: globalThis.__mutable, hits: await ctx_hits() };
});
async function ctx_hits() { return 0; }

export const persistent = http.post("/hits", { resources: [hits] }, async (ctx) => ({
  hits: await (ctx.resources["hits"] as { increment(k: string): Promise<number> }).increment("total"),
}));

export const me = http.get("/me", { auth: authenticated }, async (ctx) => ctx.auth);

export const boom = http.get("/boom", {}, async () => { throw new Error("kaboom with secret detail"); });
export const badShape = http.get("/bad-shape", { response: User }, async () => ({ id: "nope" }) as never);
export const detach = http.get("/detach", {}, async () => { setTimeout(() => {}, 5000); return { ok: true }; });
export const slow = http.get("/slow", { timeout: "200ms" }, async (ctx) => { await ctx.sleep("10s"); return { ok: true }; });
export const noContent = http.delete("/users/:id", {}, async () => http.noContent());
export const echoQuery = http.get("/echo", {}, async (ctx) => ({ query: ctx.query, headers: { "x-a": ctx.headers["x-a"] } }));

export const webhook = http.raw("/webhook", async (ctx) => {
  const bytes = await ctx.request.bytes();
  return http.rawResponse(200, `len=${bytes.length};ct=${ctx.headers["content-type"] ?? ""}`, { "x-raw": "1" });
});

export const sendReceipt = task("send-receipt", { input: z.object({ orderId: z.string() }) }, async () => {});

declare global {
  // eslint-disable-next-line no-var
  var __mutable: unknown;
}


// ---- D4/D5: tasks, cron, commands -------------------------------------
import { cron, command, dispatches } from "usai";

const audit = cache.local("audit");
type Audit = { increment(k: string): Promise<number>; get(k: string): Promise<number | null> };

export const record = task("record", { input: z.object({ what: z.string() }), resources: [audit] }, async (ctx) => {
  await (ctx.resources["audit"] as Audit).increment(ctx.input.what);
  return { recorded: ctx.input.what, sawParent: globalThis.__mutable ?? null };
});
export const slowTask = task("slow", {}, async (ctx) => { await ctx.sleep("5s"); return { done: true }; });
export const failingTask = task("failing", {}, async () => { throw errors.conflict("nope"); });
export const invokesSlow = task("invokes-slow", {}, async (ctx) => ctx.tasks.invoke(slowTask));

export const order = dispatches(
  http.post("/orders", { body: z.object({ id: z.string() }), resources: [audit] }, async (ctx) => {
    globalThis.__mutable = "parent-secret";
    const owned = await ctx.tasks.invoke(record, { what: `owned:${ctx.body.id}` });
    const { id } = await ctx.tasks.dispatch(record, { what: `dispatched:${ctx.body.id}` });
    return { owned, dispatched: id };
  }),
  record,
);
export const auditRead = http.get("/audit/:key", { resources: [audit] }, async (ctx) => ({
  value: await (ctx.resources["audit"] as Audit).get(ctx.params["key"]!),
}));
export const badDispatch = http.get("/bad-dispatch", {}, async (ctx) => {
  try { await ctx.tasks.dispatch({ name: "does-not-exist" } as never); return { ok: true }; } catch (e) { return { code: (e as { usai: { code: string } }).usai.code }; }
});

export const everySecond = cron("every-second", { schedule: "* * * * * *", resources: [audit] }, async (ctx) => {
  await (ctx.resources["audit"] as Audit).increment("cron:every-second");
  return ctx.scheduledAt;
});
export const overlapping = cron("overlapping", { schedule: "* * * * * *", overlap: "skip", resources: [audit] }, async (ctx) => {
  await (ctx.resources["audit"] as Audit).increment("cron:overlapping");
  await ctx.sleep("2500ms");
});
export const nightly = cron("nightly", { schedule: "0 3 * * *", timeout: "5m" }, async () => ({ ran: true }));

export const reconcile = command("reconcile", { resources: [audit] }, async (ctx) => {
  await (ctx.resources["audit"] as Audit).increment("command:reconcile");
  return { args: ctx.args };
});

export default defineApp({
  name: "http-fixture",
  modules: [defineModule({ name: "users", workloads: [getUser, createUser, noContent] })],
  workloads: [
    counter, persistent, me, boom, badShape, detach, slow, echoQuery, webhook, sendReceipt,
    record, slowTask, failingTask, invokesSlow, order, auditRead, badDispatch, everySecond, overlapping, nightly, reconcile,
  ],
  resources: [hits, audit],
  env: env({ GREETING: env.optional(env.string()) }),
});
