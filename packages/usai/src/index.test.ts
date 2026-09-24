import { test } from "node:test";
import assert from "node:assert/strict";

// Provide the bridge the SDK expects, as a Node stand-in.
(globalThis as unknown as { __usai: unknown }).__usai = {
  op: async () => "",
  onCancel: () => {},
  isCancelled: () => false,
};

const {
  defineApp,
  defineModule,
  http,
  task,
  cache,
  describe,
  MANIFEST_VERSION,
  errors,
  env,
  postgres,
  httpClient,
  cron,
  queue,
  command,
  auth,
  socket,
} = await import("./index.ts");

const stringSchema = {
  "~standard": {
    version: 1 as const,
    vendor: "test",
    types: undefined as unknown as { input: string; output: string },
    validate: (v: unknown) =>
      typeof v === "string" ? { value: v } : { issues: [{ message: "expected string" }] },
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
    workloads: [
      http.get(
        "/users/:id",
        { params: opaqueSchema, response: { 200: stringSchema } },
        async (ctx) => ctx.params as never,
      ),
    ],
    migrations: "./migrations/*.sql",
  });
  const cleanup = task("cleanup", { input: stringSchema, resources: [hits] }, async () => {});
  const app = defineApp({
    name: "shop",
    description: "A shop.",
    modules: [users],
    workloads: [cleanup],
    env: env({ APP_ENV: env.enum(["dev", "prod"]) }),
  });
  const m = describe(app);
  assert.equal(m.name, "shop");
  assert.equal(m.description, "A shop.");
  assert.equal(
    "description" in describe(defineApp({ name: "bare" })),
    false,
    "no description key when none is declared",
  );
  assert.deepEqual(m.modules, [{ name: "users", migrations: ["./migrations/*.sql"], seeders: [] }]);
  assert.equal(m.workloads.length, 2);
  assert.equal(m.workloads[0]!.id, "http:GET /users/:id");
  assert.equal(m.workloads[0]!.module, "users");
  assert.deepEqual(m.workloads[0]!.contracts.inWorldOnly, ["params"]);
  assert.deepEqual(m.workloads[0]!.contracts.response, { "200": { type: "string" } });
  assert.equal(m.workloads[1]!.id, "task:cleanup");
  assert.deepEqual(m.workloads[1]!.contracts.input, { type: "string" });
  assert.deepEqual(
    m.resources.map((r) => r.name),
    ["hits"],
  );
  assert.deepEqual(m.env, [
    { name: "APP_ENV", kind: "enum", required: true, values: ["dev", "prod"] },
  ]);
});

test("errors carry a stable contract", () => {
  const e = errors.notFound("no user", { id: 1 });
  assert.deepEqual(e.usai, { code: "not_found", status: 404, details: { id: 1 } });
  assert.equal(e.message, "no user");
});

test("invoke runs an http handler with parsed boundaries and encodes the response", async () => {
  const sdk = globalThis.__usai_sdk as {
    invoke(app: unknown, i: number, input: string): Promise<unknown>;
  };
  const app = defineApp({
    workloads: [
      http.post("/echo", { body: stringSchema, response: { 201: stringSchema } }, async (ctx) =>
        http.created(ctx.body.toUpperCase()),
      ),
      http.get("/plain", {}, async () => ({ ok: true })),
      http.get("/bad", { response: stringSchema }, async () => 42 as never),
    ],
  });
  const input = (i: number, body: unknown) =>
    JSON.stringify({
      kind: "http",
      env: {},
      request: {
        method: "POST",
        path: "/echo",
        url: "/echo",
        params: {},
        query: {},
        headers: {},
        body: body === undefined ? null : { json: body },
      },
    });
  assert.deepEqual(await sdk.invoke(app, 0, input(0, "hi")), {
    status: 201,
    headers: {},
    json: "HI",
  });
  assert.deepEqual(await sdk.invoke(app, 1, input(1, undefined)), {
    status: 200,
    headers: {},
    json: { ok: true },
  });
  await assert.rejects(
    sdk.invoke(app, 0, input(0, 5)),
    (e: { usai: { code: string; status: number } }) =>
      e.usai.code === "validation_failed" && e.usai.status === 400,
  );
  await assert.rejects(
    sdk.invoke(app, 2, input(2, undefined)),
    (e: { usai: { code: string } }) => e.usai.code === "response_contract_violation",
  );
});

test("a hole in a declaration list is named, not an undefined crash", () => {
  const late = undefined as unknown as ReturnType<typeof http.get>;
  const app = defineApp({
    name: "holes",
    modules: [defineModule({ name: "m", workloads: [late] })],
  });
  assert.throws(
    () => describe(app),
    /module "m": workloads\[0\] is undefined — is it declared after/,
  );
  const undeclaredResources = defineApp({ name: "holes2", workloads: [late] });
  assert.throws(() => describe(undeclaredResources), /app "holes2": workloads\[0\] is undefined/);
});

test("an undeclared resource is a named error", async () => {
  const { makeResources } = await import("./runtime/context.ts");
  const resources = makeResources([]);
  assert.throws(
    () => (resources as Record<string, unknown>)["main"],
    (e: unknown) => (e as { usai?: { code: string } }).usai?.code === "resource_not_declared",
  );
  assert.equal("then" in resources, false);
  assert.equal(JSON.stringify(resources), "{}");
});

test("ctx.resources is typed from the declaration list", () => {
  const db = postgres("db");
  const mailer = httpClient("mailer", { baseUrl: "https://mail.example" });
  const hits = cache.local("hits");
  // Compile-time: each handle has its own methods, and an undeclared name
  // is a type error (checked by the `@ts-expect-error` lines).
  const w = http.get(
    "/x",
    {
      resources: [db, mailer, hits],
      summary: "one of each",
      description: "The three handle kinds.",
    },
    async (ctx) => {
      const rows: Array<{ n: number }> = await ctx.resources.db.query<{ n: number }>(
        "select 1 as n",
      );
      const res = await ctx.resources.mailer.fetch("/send", { json: rows });
      const count = await ctx.resources.hits.increment("total");
      // @ts-expect-error not declared on this workload
      ctx.resources.other;
      return { ok: res.ok, count };
    },
  );
  assert.equal(w.summary, "one of each");
  assert.equal(w.description, "The three handle kinds.");
  const tick = cron("tick", { schedule: "* * * * *", resources: [hits] }, async (ctx) =>
    ctx.resources.hits.clear(),
  );
  assert.equal(tick.kind, "cron");
  const consumer = queue.consume("t", { resources: [db], description: "d" }, async (ctx) =>
    ctx.resources.db.execute("select 1"),
  );
  assert.equal(consumer.description, "d");
  const bare = task("bare", {}, async (ctx) => {
    // Without a list the map is open: `unknown` values, no type error.
    const anything: unknown = ctx.resources["whatever"];
    return anything;
  });
  assert.equal(bare.kind, "task");
});

test("a declared admission budget reaches the manifest for every kind that takes one", () => {
  // `command()` accepted `concurrency:` and never forwarded it: the type
  // said the budget existed, the manifest carried nothing, and `usai
  // inspect` printed no budget where every other kind printed one. Two
  // `usai app import` runs at once were bounded only by the application's
  // budget.
  const m = describe(
    defineApp({
      name: "budgets",
      workloads: [
        command("import", { concurrency: 1, timeout: "5m" }, async () => {}),
        task("resize", { concurrency: 3 }, async () => {}),
        cron("sweep", { schedule: "*/5 * * * *", timeout: "90s" }, async () => {}),
      ],
    }),
  );
  const byId = Object.fromEntries(m.workloads.map((w) => [w.id, w]));
  assert.equal(byId["command:import"]!.maxConcurrency, 1);
  assert.equal(byId["command:import"]!.timeoutMs, 300_000);
  assert.equal(byId["task:resize"]!.maxConcurrency, 3);
  // A cron's deadline is the workload's, written once. The trigger used to
  // carry a second copy that the runtime never read.
  assert.equal(byId["cron:sweep"]!.timeoutMs, 90_000);
  assert.equal(
    "timeoutMs" in (byId["cron:sweep"]!.trigger as Record<string, unknown>),
    false,
    "the schedule states no second deadline",
  );
});

test("a module reached twice is one module, and two of the same name is an error", () => {
  // The diamond every shared layer grows: `auth` and `billing` both
  // re-export `base`, and the application lists all three. It was a hard
  // `duplicate workload id`, with no de-duplication even for the identical
  // `ModuleDeclaration` object — a structural limit on how deep a shared
  // layer can go.
  const base = defineModule({ name: "base", workloads: [task("ping", {}, async () => 1)] });
  const app = defineApp({ name: "diamond", modules: [base, base] });
  assert.equal(describe(app).workloads.length, 1);
  assert.deepEqual(describe(app).modules, [{ name: "base", migrations: [], seeders: [] }]);
  // Two *different* modules with one name was accepted in silence, and
  // `inspect` then attributed workloads to an ambiguous label.
  assert.throws(
    () =>
      describe(
        defineApp({
          name: "twins",
          modules: [
            defineModule({ name: "same", workloads: [task("a", {}, async () => 1)] }),
            defineModule({ name: "same", workloads: [task("b", {}, async () => 1)] }),
          ],
        }),
      ),
    /two different modules are both called "same"/,
  );
});

test("a duplicate workload id names who declared it", () => {
  // The runtime refuses it at install by id alone. Here the owners are
  // still known, and with two vendored module packages that is the
  // difference between a grep and a glance.
  assert.throws(
    () =>
      describe(
        defineApp({
          name: "collide",
          modules: [
            defineModule({ name: "auth", workloads: [task("cleanup", {}, async () => 1)] }),
            defineModule({ name: "billing", workloads: [task("cleanup", {}, async () => 1)] }),
          ],
        }),
      ),
    /duplicate workload id task:cleanup: declared by module "auth" and by module "billing"/,
  );
  assert.throws(
    () =>
      describe(
        defineApp({
          name: "collide2",
          modules: [defineModule({ name: "m", workloads: [task("x", {}, async () => 1)] })],
          workloads: [task("x", {}, async () => 1)],
        }),
      ),
    /declared by module "m" and by the application/,
  );
});

test("two auth schemes with one name is an error, not a silent merge", () => {
  // The name *is* the OpenAPI security scheme. Two modules each calling
  // theirs `session` used to pass in silence: the first one seen described
  // both, so a cookie scheme was documented as HTTP bearer and every
  // generated client for the second route sent `Authorization: Bearer` and
  // got 401 forever. The resolvers ran correctly — which is what made it
  // invisible.
  const bearer = auth.bearer({
    name: "session",
    description: "module one",
    resolve: async () => ({ userId: "x" }),
  });
  const cookie = auth.cookie({
    name: "session",
    cookie: "sid",
    description: "module two",
    resolve: async () => ({ userId: "x" }),
  });
  assert.throws(
    () =>
      describe(
        defineApp({
          name: "schemes",
          modules: [
            defineModule({
              name: "one",
              workloads: [http.get("/one", { auth: bearer }, async () => ({}))],
            }),
            defineModule({
              name: "two",
              workloads: [http.get("/two", { auth: cookie }, async () => ({}))],
            }),
          ],
        }),
      ),
    /auth scheme "session" is declared twice with different configuration \(module "one" and module "two"\)/,
  );
  // One declaration reused by reference is the documented shape and stays fine.
  const shared = describe(
    defineApp({
      name: "shared-scheme",
      modules: [
        defineModule({
          name: "one",
          workloads: [http.get("/one", { auth: bearer }, async () => ({}))],
        }),
        defineModule({
          name: "two",
          workloads: [http.get("/two", { auth: bearer }, async () => ({}))],
        }),
      ],
    }),
  );
  assert.equal(shared.auth.length, 1);
});

test("a module declares what it needs from the environment", () => {
  // A module had no way to say this, so every consuming application had to
  // mirror the variable into its own `env({})` by hand and nothing checked
  // that it had: `ctx.env.BILLING_TAX_RATE` was `undefined` even with the
  // variable set in the process environment, and the failure was a 500 on
  // the first request that reached the module — not at build, not at
  // activation.
  const billing = defineModule({
    name: "billing",
    env: env({ BILLING_TAX_RATE: env.string(), BILLING_CURRENCY: env.enum(["IDR", "USD"]) }),
    workloads: [task("issue", {}, async () => 1)],
  });
  const m = describe(defineApp({ name: "shop", modules: [billing] }));
  assert.deepEqual(m.env.map((e) => e.name).sort(), ["BILLING_CURRENCY", "BILLING_TAX_RATE"]);
  // The application's own declaration of a key wins.
  const tightened = describe(
    defineApp({
      name: "shop2",
      modules: [billing],
      env: env({ BILLING_TAX_RATE: env.optional(env.string()) }),
    }),
  );
  assert.equal(tightened.env.find((e) => e.name === "BILLING_TAX_RATE")?.required, false);
  // Two modules declaring one key differently would mean one silently takes
  // the other's parse.
  const other = defineModule({
    name: "other",
    env: env({ BILLING_TAX_RATE: env.int() }),
    workloads: [task("x", {}, async () => 1)],
  });
  assert.throws(
    () => describe(defineApp({ name: "shop3", modules: [billing, other] })),
    /BILLING_TAX_RATE is declared differently by module "billing" and module "other"/,
  );
});

test("a body bound on a kind that has no body fails the build, not the type only", () => {
  // The compiler refuses this, and the release note promised the *build*
  // refuses it too — "with exit 1 and the workload named … rather than being
  // accepted and ignored". It was not true: the only enforcement was the
  // type, and every way past it (`usai test --no-typecheck`, an `as never`, a
  // JavaScript project) reached a constructor that copied `timeout` and
  // `concurrency` into the policies and dropped `maxBodyBytes` on the floor.
  // It never reached the manifest, so the definition — which does refuse it —
  // had nothing to refuse, and the declaration was accepted and ignored: the
  // exact outcome the option was added to prevent.
  const bodyless: [string, () => unknown][] = [
    ["task", () => task("t", { maxBodyBytes: 1024 } as never, async () => 1)],
    [
      "cron",
      () => cron("c", { schedule: "* * * * *", maxBodyBytes: 1024 } as never, async () => {}),
    ],
    ["command", () => command("c", { maxBodyBytes: 1024 } as never, async () => {})],
    ["stream", () => http.stream("/s", { maxBodyBytes: 1024 } as never, async () => {})],
    ["socket", () => socket("/s", { maxBodyBytes: 1024 } as never, { open: async () => {} })],
    ["queue consumer", () => queue.consume("t", { maxBodyBytes: 1024 } as never, async () => {})],
  ];
  for (const [kind, declare] of bodyless) {
    assert.throws(
      declare,
      (e: Error) => {
        assert.match(e.message, /maxBodyBytes/, `${kind}: ${e.message}`);
        assert.match(e.message, /no request body/, `${kind}: ${e.message}`);
        return true;
      },
      `${kind} accepted a body bound it has no body for`,
    );
  }
  // The kind that does have a body still takes it, and it still reaches the
  // manifest — a refusal that also broke the working case would be worse.
  const route = http.post(
    "/upload",
    { body: opaqueSchema, response: { 200: opaqueSchema }, maxBodyBytes: 4096 },
    async () => ({}),
  );
  const manifest = describe(defineApp({ name: "b", workloads: [route] }));
  assert.equal(manifest.workloads[0]?.maxBodyBytes, 4096);
});
