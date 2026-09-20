// HTTP fixture for the runtime's D2 acceptance tests. Every endpoint exists
// to prove one contract; see crates/usai-runtime/tests/http.rs.
import {
  defineApp,
  defineModule,
  http,
  auth,
  cookies,
  cache,
  errors,
  env,
  task,
  UsaiError,
} from "@sakaladev/usai";
import { z } from "zod";

const Params = z.object({ id: z.string().uuid() });
const User = z.object({ id: z.string().uuid(), name: z.string(), email: z.string().email() });
const Query = z.object({
  page: z.number().int().min(1).default(1),
  tags: z.array(z.string()).optional(),
});
const Body = z.object({ name: z.string().min(1), email: z.string().email() });

const hits = cache.local("hits");

const authenticated = auth.bearer({
  name: "token",
  resolve: async (_ctx, token) => {
    if (token !== "secret") throw errors.unauthorized("bad token");
    return { userId: "u1" };
  },
});

export const getUser = http.get(
  "/users/:id",
  {
    summary: "One user",
    description: "By id, with the page the caller asked for.",
    params: Params,
    query: Query,
    response: { 200: User },
    errors: [{ code: "not_found", status: 404 }],
  },
  async (ctx) => {
    if (ctx.params.id === "00000000-0000-0000-0000-000000000000")
      throw errors.notFound("user not found", { id: ctx.params.id });
    return { id: ctx.params.id, name: `user page ${ctx.query.page}`, email: "a@b.co" };
  },
);

export const createUser = http.post(
  "/users",
  { body: Body, response: { 201: User } },
  async (ctx) => http.created({ id: "6f1a2b3c-4d5e-4f60-8a71-92b3c4d5e6f7", ...ctx.body }),
);

// Validate once: a structural body is final at the boundary (the world
// only strips undeclared keys); a transform keeps the in-world parse.
export const shape = http.post(
  "/shape",
  { body: z.object({ a: z.string(), n: z.number().optional() }) },
  async (ctx) => ({ keys: Object.keys(ctx.body) }),
);
export const shaped = http.post(
  "/shape-transform",
  { body: z.object({ a: z.string().transform((v) => v.toUpperCase()) }) },
  async (ctx) => ({ a: ctx.body.a }),
);

export const counter = http.get("/counter", {}, async () => {
  globalThis.__mutable = ((globalThis.__mutable as number | undefined) ?? 0) + 1;
  return { counter: globalThis.__mutable, hits: await ctx_hits() };
});
async function ctx_hits() {
  return 0;
}

export const persistent = http.post("/hits", { resources: [hits] }, async (ctx) => ({
  hits: await (ctx.resources["hits"] as { increment(k: string): Promise<number> }).increment(
    "total",
  ),
}));

export const me = http.get("/me", { auth: authenticated }, async (ctx) => ctx.auth);
export const requestId = http.get("/request-id", {}, async (ctx) => ({ id: ctx.requestId }));
export const framed = http.get("/framed", {}, async () =>
  http.response(200, { ok: true }, { "x-frame-options": "SAMEORIGIN" }),
);

// A session cookie: the cookie scheme hands the cookie's value to the
// resolver and the OpenAPI document says `apiKey in: cookie`; login sets two
// cookies at once (a repeated header); a custom scheme that declares nothing
// gets no invented security scheme.
const session = auth.cookie({
  name: "session",
  description: "the sid cookie set by /login",
  cookie: "sid",
  resolve: async (_ctx, sid) => {
    if (sid !== "s3ss10n") throw errors.unauthorized("no session");
    return { userId: "u1" };
  },
});
export const meByCookie = http.get("/me/cookie", { auth: session }, async (ctx) => ctx.auth);
export const login = http.post("/login", {}, async () =>
  http.response(
    200,
    { ok: true },
    {
      "set-cookie": [
        cookies.serialize("sid", "s3ss10n", { maxAge: 3600 }),
        cookies.serialize("theme", "dark", { httpOnly: false }),
      ],
    },
  ),
);
const opaque = auth.custom({
  name: "opaque",
  resolve: async () => ({ userId: "anyone" }),
});
export const meOpaque = http.get("/me/opaque", { auth: opaque }, async (ctx) => ctx.auth);

// An operation's own failure surfacing through the handler (the shape a
// PostgreSQL error has): the SQLSTATE is for the log, the client gets
// `internal`.
export const boomOperation = http.get("/boom-operation", {}, async () => {
  throw new UsaiError("sql_22003", 500, "integer out of range");
});
export const boom = http.get("/boom", {}, async () => {
  throw new Error("kaboom with secret detail");
});
export const badShape = http.get(
  "/bad-shape",
  { response: User },
  async () => ({ id: "nope" }) as never,
);
export const detach = http.get("/detach", {}, async () => {
  setTimeout(() => {}, 5000);
  return { ok: true };
});
// A write that was not awaited: the handler answers success over an
// operation the runtime is about to cancel — that answer must not commit.
export const detachWrite = http.post("/detach-write", { resources: [hits] }, async (ctx) => {
  void (ctx.resources["hits"] as { increment(k: string): Promise<number> }).increment("detached");
  return { ok: true };
});
export const slow = http.get("/slow", { timeout: "200ms" }, async (ctx) => {
  await ctx.sleep("10s");
  return { ok: true };
});
export const noContent = http.delete("/users/:id", {}, async () => http.noContent());
// Using a resource without declaring it is named, not `undefined`.
export const undeclared = http.get("/undeclared", {}, async (ctx) => ({
  n: await (ctx.resources["hits"] as { get(k: string): Promise<unknown> }).get("x"),
}));
// A deadline must end synchronous work too, not only awaiting handlers.
export const busy = http.get("/busy", { timeout: "300ms" }, async () => {
  const end = Date.now() + 5000;
  let i = 0;
  while (Date.now() < end) i++;
  return { i };
});
export const echoQuery = http.get("/echo", {}, async (ctx) => ({
  query: ctx.query,
  headers: { "x-a": ctx.headers["x-a"] },
}));

// Decode a large body in one call: the bridge's TextDecoder/atob/btoa build
// strings in blocks (a 2 MB decode used to exceed the CPU slice).
export const decodeBig = http.raw("/decode", { method: "POST" }, async (ctx) => {
  const bytes = await ctx.request.bytes();
  const text = new TextDecoder().decode(bytes);
  const b64 = btoa(text.slice(0, 1_000_000));
  const back = atob(b64);
  return http.rawResponse(
    200,
    JSON.stringify({ bytes: bytes.length, chars: text.length, roundtrip: back.length }),
    { "content-type": "application/json" },
  );
});
export const webhook = http.raw(
  "/webhook",
  { responses: { 200: "echo of the body length", 401: "bad signature" } },
  async (ctx) => {
    const bytes = await ctx.request.bytes();
    return http.rawResponse(200, `len=${bytes.length};ct=${ctx.headers["content-type"] ?? ""}`, {
      "x-raw": "1",
    });
  },
);

export const sendReceipt = task(
  "send-receipt",
  { input: z.object({ orderId: z.string() }) },
  async () => {},
);

declare global {
  // eslint-disable-next-line no-var
  var __mutable: unknown;
  // eslint-disable-next-line no-var
  var __serviceLocal: unknown;
  // eslint-disable-next-line no-var
  var __socketLocal: unknown;
}

// ---- D4/D5: tasks, cron, commands -------------------------------------
import { cron, command, dispatches } from "@sakaladev/usai";

const audit = cache.local("audit");
type Audit = {
  increment(k: string): Promise<number>;
  get(k: string): Promise<number | null>;
  set(k: string, v: unknown): Promise<boolean>;
};

export const record = task(
  "record",
  { input: z.object({ what: z.string() }), resources: [audit] },
  async (ctx) => {
    await (ctx.resources["audit"] as Audit).increment(ctx.input.what);
    return { recorded: ctx.input.what, sawParent: globalThis.__mutable ?? null };
  },
);
export const slowTask = task("slow", {}, async (ctx) => {
  await ctx.sleep("5s");
  return { done: true };
});
export const failingTask = task("failing", {}, async () => {
  throw errors.conflict("nope");
});
export const invokesSlow = task("invokes-slow", {}, async (ctx) => ctx.tasks.invoke(slowTask));
// One at a time: a second invoke while the first holds the slot is refused
// with `capacity_exhausted` in the caller's world.
export const single = task("single", { concurrency: 1 }, async (ctx) => {
  await ctx.sleep("1s");
  return { done: true };
});
export const invokesSingle = task("invokes-single", {}, async (ctx) => {
  const first = ctx.tasks.invoke(single);
  await ctx.sleep("100ms");
  try {
    await ctx.tasks.invoke(single);
    return { second: "ran" };
  } catch (e) {
    return { second: (e as { usai?: { code: string; status: number } }).usai };
  } finally {
    await first;
  }
});

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
  try {
    await ctx.tasks.dispatch({ name: "does-not-exist" } as never);
    return { ok: true };
  } catch (e) {
    return { code: (e as { usai: { code: string } }).usai.code };
  }
});

export const everySecond = cron(
  "every-second",
  { schedule: "* * * * * *", resources: [audit] },
  async (ctx) => {
    await (ctx.resources["audit"] as Audit).increment("cron:every-second");
    return ctx.scheduledAt;
  },
);
export const overlapping = cron(
  "overlapping",
  { schedule: "* * * * * *", overlap: "skip", resources: [audit] },
  async (ctx) => {
    await (ctx.resources["audit"] as Audit).increment("cron:overlapping");
    await ctx.sleep("2500ms");
  },
);
export const nightly = cron("nightly", { schedule: "0 3 * * *", timeout: "5m" }, async () => ({
  ran: true,
}));

// ---- D9: services ---------------------------------------------------------
import { service } from "@sakaladev/usai";

export const ledgerSync = service("ledger-sync", { resources: [audit] }, async (ctx) => {
  // Mutable state that survives iterations because the service is alive.
  const state = new Map<string, number>();
  globalThis.__serviceLocal = "service-secret";
  let iterations = 0;
  while (!ctx.signal.aborted) {
    iterations += 1;
    state.set("iterations", iterations);
    await (ctx.resources["audit"] as Audit).increment("service:iterations");
    await ctx.sleep("100ms");
  }
  await (ctx.resources["audit"] as Audit).set("service:final", state.get("iterations") ?? 0);
  return { iterations, reason: ctx.signal.reason };
});

export const serviceLocalRead = http.get("/service-local", {}, async () => ({
  sees: globalThis.__serviceLocal ?? null,
}));

// ---- D13: hardening ---------------------------------------------------------
export const memoryHog = task("memory-hog", {}, async () => {
  const chunks: string[] = [];
  for (;;) chunks.push("x".repeat(1024 * 1024));
});

export const crashy = service(
  "crashy",
  { restart: { mode: "on-failure", backoffMs: 20, maxRestarts: 3 }, resources: [audit] },
  async (ctx) => {
    const n = await (ctx.resources["audit"] as Audit).increment("service:crashy:starts");
    if (n <= 2) throw errors.internal(`crash ${n}`);
    while (!ctx.signal.aborted) await ctx.sleep("50ms");
    return { survived: true };
  },
);

// ---- D11: streams and sockets ---------------------------------------------
import { socket } from "@sakaladev/usai";

export const events = http.stream(
  "/events",
  { query: z.object({ n: z.coerce.number().int().min(1).max(100).default(3) }) },
  async (ctx, stream) => {
    await stream.start({ headers: { "x-stream": "yes" } });
    for (let i = 1; i <= (ctx.query as unknown as { n: number }).n; i++) {
      await stream.event("tick", { i });
      await ctx.sleep("20ms");
    }
    await stream.event("done", { total: (ctx.query as unknown as { n: number }).n });
  },
);

export const endless = http.stream("/endless", {}, async (ctx, stream) => {
  let i = 0;
  while (!ctx.signal.aborted) {
    await stream.send(`chunk ${i++}\n`);
    await ctx.sleep("30ms");
  }
  return { stopped: true, chunks: i };
});

export const plainStream = http.stream("/no-send", {}, async () => ({ nothing: "sent" }));

// An authenticated socket: the resolver runs before the upgrade completes,
// so a refusal is a 401, and a browser passes the token as the second
// entry of `Sec-WebSocket-Protocol` ("bearer", token).
export const privateChat = socket(
  "/private-chat",
  { auth: authenticated, outgoing: z.object({ user: z.string() }) },
  {
    async open(ctx) {
      await ctx.send({ user: (ctx.auth as { userId: string }).userId });
    },
  },
);

export const chat = socket(
  "/chat",
  {
    incoming: z.object({ text: z.string() }),
    outgoing: z.object({ echo: z.string(), count: z.number() }),
    resources: [audit],
  },
  {
    async open(ctx) {
      ctx.state["count"] = 0;
      ctx.state["user"] = ctx.query["user"] ?? "anon";
      globalThis.__socketLocal = "socket-secret";
    },
    async message(ctx) {
      ctx.state["count"] = (ctx.state["count"] as number) + 1;
      await ctx.send({
        echo: `${String(ctx.state["user"])}: ${ctx.message.text}`,
        count: ctx.state["count"] as number,
      });
      if (ctx.message.text === "bye") await ctx.close("bye then");
    },
    async close(ctx) {
      await (ctx.resources["audit"] as Audit).set(
        `socket:${String(ctx.state["user"])}`,
        ctx.state["count"],
      );
    },
  },
);
export const socketLocalRead = http.get("/socket-local", {}, async () => ({
  sees: globalThis.__socketLocal ?? null,
}));

export const reconcile = command("reconcile", { resources: [audit] }, async (ctx) => {
  await (ctx.resources["audit"] as Audit).increment("command:reconcile");
  return { args: ctx.args };
});

// Web platform globals inside a world: `z.string().url()` validates in the
// world through `new URL`, and handlers use URL / structuredClone.
export const inspectUrl = http.post(
  "/url",
  { body: z.object({ url: z.string().url() }) },
  async (ctx) => {
    const u = new URL(ctx.body.url);
    u.searchParams.set("seen", "1");
    const copy = structuredClone({ when: new Date(0), tags: new Set(["a"]) });
    return {
      host: u.host,
      path: u.pathname,
      href: u.href,
      cloned: copy.when instanceof Date && copy.tags.has("a"),
    };
  },
);

// ---- P4: outbound HTTP, crypto, password ---------------------------------
import { httpClient, password } from "@sakaladev/usai";
const upstream = httpClient("upstream", {
  baseUrlEnv: "UPSTREAM_URL",
  timeoutMs: 2000,
  maxConcurrent: 2,
});
type Client = import("@sakaladev/usai").HttpClientHandle;
const up = (ctx: { resources: Record<string, unknown> }) => ctx.resources["upstream"] as Client;

export const egress = http.post(
  "/egress",
  {
    body: z.object({
      path: z.string(),
      method: z.string().optional(),
      json: z.unknown().optional(),
    }),
    resources: [upstream],
  },
  async (ctx) => {
    const res = await up(ctx).fetch(ctx.body.path, {
      method: ctx.body.method ?? "GET",
      json: ctx.body.json,
    });
    return {
      status: res.status,
      ok: res.ok,
      echo: res.headers["x-echo"],
      body: res.status === 200 ? res.json() : res.text(),
    };
  },
);
export const egressOther = http.get("/egress/other", { resources: [upstream] }, async (ctx) => {
  try {
    await up(ctx).fetch("https://example.com/");
    return { unexpected: true };
  } catch (e) {
    return { code: (e as { usai: { code: string } }).usai.code };
  }
});
export const egressSlow = http.get(
  "/egress/slow",
  { timeout: "300ms", resources: [upstream] },
  async (ctx) => {
    await up(ctx).fetch("/slow");
    return { unreachable: true };
  },
);
export const egressBytes = http.get("/egress/bytes", { resources: [upstream] }, async (ctx) => {
  const res = await up(ctx).fetch("/bytes");
  return { length: res.bytes().length, first: res.bytes()[0] };
});
export const noFetch = http.get("/nofetch", {}, async () => {
  try {
    await fetch("https://example.com/");
    return { unexpected: true };
  } catch (e) {
    return { code: (e as { usai: { code: string } }).usai.code, message: (e as Error).message };
  }
});
export const cryptoRoute = http.get("/crypto", {}, async () => {
  const enc = new TextEncoder();
  const hex = (b: ArrayBuffer) =>
    Array.from(new Uint8Array(b), (x) => x.toString(16).padStart(2, "0")).join("");
  const key = await crypto.subtle.importKey(
    "raw",
    enc.encode("k"),
    { name: "HMAC", hash: "SHA-256" },
    false,
    ["sign", "verify"],
  );
  const sig = await crypto.subtle.sign("HMAC", key, enc.encode("abc"));
  return {
    uuid: crypto.randomUUID(),
    other: crypto.randomUUID(),
    sha256: hex(await crypto.subtle.digest("SHA-256", enc.encode("abc"))),
    hmac: hex(sig),
    verified: await crypto.subtle.verify("HMAC", key, sig, enc.encode("abc")),
    random: Array.from(crypto.getRandomValues(new Uint8Array(4))),
  };
});
export const passwordRoute = http.post(
  "/password",
  { body: z.object({ password: z.string() }) },
  async (ctx) => {
    const hash = await password.hash(ctx.body.password);
    return {
      prefix: hash.slice(0, 10),
      ok: await password.verify(ctx.body.password, hash),
      wrong: await password.verify("nope", hash),
    };
  },
);

export default defineApp({
  name: "http-fixture",
  // Set on every application response; a handler's own header wins.
  headers: { "X-Content-Type-Options": "nosniff", "x-frame-options": "DENY" },
  description: "The HTTP test fixture: one of everything the pipeline can serve.",
  modules: [defineModule({ name: "users", workloads: [getUser, createUser, noContent] })],
  workloads: [
    counter,
    persistent,
    me,
    requestId,
    framed,
    decodeBig,
    meByCookie,
    login,
    meOpaque,
    boom,
    badShape,
    detach,
    detachWrite,
    slow,
    echoQuery,
    webhook,
    sendReceipt,
    record,
    slowTask,
    single,
    invokesSingle,
    privateChat,
    boomOperation,
    failingTask,
    invokesSlow,
    order,
    auditRead,
    badDispatch,
    everySecond,
    overlapping,
    nightly,
    reconcile,
    ledgerSync,
    serviceLocalRead,
    events,
    endless,
    plainStream,
    chat,
    socketLocalRead,
    memoryHog,
    crashy,
    inspectUrl,
    busy,
    undeclared,
    egress,
    egressOther,
    egressSlow,
    egressBytes,
    noFetch,
    cryptoRoute,
    passwordRoute,
    shape,
    shaped,
  ],
  resources: [hits, audit, upstream],
  env: env({ GREETING: env.optional(env.string()), UPSTREAM_URL: env.url() }),
});
