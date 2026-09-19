import { test } from "node:test";
import assert from "node:assert/strict";

// Provide the bridge the SDK expects, as a Node stand-in.
(globalThis as unknown as { __usai: unknown }).__usai = {
  op: async () => "",
  onCancel: () => {},
  isCancelled: () => false,
};

const { defineApp, defineModule, http, task, cache, describe, MANIFEST_VERSION, errors, env, postgres, httpClient, cron, queue } = await import("./index.ts");

const stringSchema = {
  "~standard": {
    version: 1 as const,
    vendor: "test",
    types: undefined as unknown as { input: string; output: string },
    validate: (v: unknown) => (typeof v === "string" ? { value: v } : { issues: [{ message: "expected string" }] }),
    jsonSchema: { input: () => ({ type: "string" }), output: () => ({ type: "string" }) },
  },
};
const opaqueSchema = {
  "~standard": { version: 1 as const, vendor: "test", validate: (v: unknown) => ({ value: v }) },
};

test("manifest version matches the runtime", () => {
  assert.equal(MANIFEST_VERSION, 1);
});

test("describe flattens modules deterministically and extracts JSON Schema", () => {
  const hits = cache.local("hits");
  const users = defineModule({
    name: "users",
    workloads: [http.get("/users/:id", { params: opaqueSchema, response: { 200: stringSchema } }, async (ctx) => ctx.params as never)],
    migrations: "./migrations/*.sql",
  });
  const cleanup = task("cleanup", { input: stringSchema, resources: [hits] }, async () => {});
  const app = defineApp({ name: "shop", description: "A shop.", modules: [users], workloads: [cleanup], env: env({ APP_ENV: env.enum(["dev", "prod"]) }) });
  const m = describe(app);
  assert.equal(m.name, "shop");
  assert.equal(m.description, "A shop.");
  assert.equal("description" in describe(defineApp({ name: "bare" })), false, "no description key when none is declared");
  assert.deepEqual(m.modules, [{ name: "users", migrations: ["./migrations/*.sql"], seeders: [] }]);
  assert.equal(m.workloads.length, 2);
  assert.equal(m.workloads[0]!.id, "http:GET /users/:id");
  assert.equal(m.workloads[0]!.module, "users");
  assert.deepEqual(m.workloads[0]!.contracts.inWorldOnly, ["params"]);
  assert.deepEqual(m.workloads[0]!.contracts.response, { "200": { type: "string" } });
  assert.equal(m.workloads[1]!.id, "task:cleanup");
  assert.deepEqual(m.workloads[1]!.contracts.input, { type: "string" });
  assert.deepEqual(m.resources.map((r) => r.name), ["hits"]);
  assert.deepEqual(m.env, [{ name: "APP_ENV", kind: "enum", required: true, values: ["dev", "prod"] }]);
});

test("errors carry a stable contract", () => {
  const e = errors.notFound("no user", { id: 1 });
  assert.deepEqual(e.usai, { code: "not_found", status: 404, details: { id: 1 } });
  assert.equal(e.message, "no user");
});

test("invoke runs an http handler with parsed boundaries and encodes the response", async () => {
  const sdk = globalThis.__usai_sdk as { invoke(app: unknown, i: number, input: string): Promise<unknown> };
  const app = defineApp({
    workloads: [
      http.post("/echo", { body: stringSchema, response: { 201: stringSchema } }, async (ctx) => http.created(ctx.body.toUpperCase())),
      http.get("/plain", {}, async () => ({ ok: true })),
      http.get("/bad", { response: stringSchema }, async () => 42 as never),
    ],
  });
  const input = (i: number, body: unknown) => JSON.stringify({ kind: "http", env: {}, request: { method: "POST", path: "/echo", url: "/echo", params: {}, query: {}, headers: {}, body: body === undefined ? null : { json: body } } });
  assert.deepEqual(await sdk.invoke(app, 0, input(0, "hi")), { status: 201, headers: {}, json: "HI" });
  assert.deepEqual(await sdk.invoke(app, 1, input(1, undefined)), { status: 200, headers: {}, json: { ok: true } });
  await assert.rejects(sdk.invoke(app, 0, input(0, 5)), (e: { usai: { code: string; status: number } }) => e.usai.code === "validation_failed" && e.usai.status === 400);
  await assert.rejects(sdk.invoke(app, 2, input(2, undefined)), (e: { usai: { code: string } }) => e.usai.code === "response_contract_violation");
});

test("a hole in a declaration list is named, not an undefined crash", () => {
  const late = undefined as unknown as ReturnType<typeof http.get>;
  const app = defineApp({ name: "holes", modules: [defineModule({ name: "m", workloads: [late] })] });
  assert.throws(() => describe(app), /module "m": workloads\[0\] is undefined — is it declared after/);
  const undeclaredResources = defineApp({ name: "holes2", workloads: [late] });
  assert.throws(() => describe(undeclaredResources), /app "holes2": workloads\[0\] is undefined/);
});

test("an undeclared resource is a named error", async () => {
  const { makeResources } = await import("./runtime/context.ts");
  const resources = makeResources([]);
  assert.throws(() => (resources as Record<string, unknown>)["main"], (e: unknown) => (e as { usai?: { code: string } }).usai?.code === "resource_not_declared");
  assert.equal("then" in resources, false);
  assert.equal(JSON.stringify(resources), "{}");
});

test("ctx.resources is typed from the declaration list", () => {
  const db = postgres("db");
  const mailer = httpClient("mailer", { baseUrl: "https://mail.example" });
  const hits = cache.local("hits");
  // Compile-time: each handle has its own methods, and an undeclared name
  // is a type error (checked by the `@ts-expect-error` lines).
  const w = http.get("/x", { resources: [db, mailer, hits], summary: "one of each", description: "The three handle kinds." }, async (ctx) => {
    const rows: Array<{ n: number }> = await ctx.resources.db.query<{ n: number }>("select 1 as n");
    const res = await ctx.resources.mailer.fetch("/send", { json: rows });
    const count = await ctx.resources.hits.increment("total");
    // @ts-expect-error not declared on this workload
    ctx.resources.other;
    return { ok: res.ok, count };
  });
  assert.equal(w.summary, "one of each");
  assert.equal(w.description, "The three handle kinds.");
  const tick = cron("tick", { schedule: "* * * * *", resources: [hits] }, async (ctx) => ctx.resources.hits.clear());
  assert.equal(tick.kind, "cron");
  const consumer = queue.consume("t", { resources: [db], description: "d" }, async (ctx) => ctx.resources.db.execute("select 1"));
  assert.equal(consumer.description, "d");
  const bare = task("bare", {}, async (ctx) => {
    // Without a list the map is open: `unknown` values, no type error.
    const anything: unknown = ctx.resources["whatever"];
    return anything;
  });
  assert.equal(bare.kind, "task");
});

