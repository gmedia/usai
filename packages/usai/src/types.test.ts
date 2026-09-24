// The declaration surfaces are typed so that a typo in an option key is a
// compile error. That is not automatic: where the options parameter is the
// inferred type itself (HTTP routes and streams, so that `ctx` and the
// response type can be read off it), TypeScript's excess-property check does
// not fire and an unknown key simply widens the inferred type — `Auth:` for
// `auth:` compiled, built and served a route without its authentication
// (found by an external round on 0.0.7). `NoExtraKeys` puts the check back;
// this test is what keeps it there.
import { test } from "node:test";
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const sdk = resolve(here, "index.ts");
const tsc = resolve(here, "..", "node_modules", ".bin", "tsc");

/** Type-checks `source` against the SDK's own sources; returns tsc's output. */
function check(source: string): string {
  const dir = mkdtempSync(join(tmpdir(), "usai-types-"));
  writeFileSync(join(dir, "t.ts"), source);
  writeFileSync(
    join(dir, "tsconfig.json"),
    JSON.stringify({
      compilerOptions: {
        strict: true,
        module: "esnext",
        moduleResolution: "bundler",
        target: "es2022",
        noEmit: true,
        types: [],
        allowImportingTsExtensions: true,
        paths: { "@sakaladev/usai": [sdk], zod: [resolve(here, "..", "node_modules", "zod")] },
      },
      files: ["t.ts"],
    }),
  );
  try {
    execFileSync(tsc, ["-p", join(dir, "tsconfig.json")], { encoding: "utf8" });
    return "";
  } catch (error) {
    const e = error as { stdout?: string; stderr?: string };
    return `${e.stdout ?? ""}${e.stderr ?? ""}`;
  }
}

const preamble = `import { http, auth, postgres } from "@sakaladev/usai";
import { z } from "zod";
const db = postgres("main", {});
const session = auth.bearer({ name: "session", resolve: async () => ({ userId: "x" }) });
const Out = z.object({ n: z.number() });
`;

test("a misspelled option key on a route is a compile error", () => {
  const out = check(
    `${preamble}export const r = http.get("/a", { response: Out, Auth: session }, async () => ({ n: 1 }));`,
  );
  assert.match(out, /t\.ts\(6,\d+\)/, `the error points at the key: ${out}`);
  assert.match(out, /never/, out);
});

test("an unknown option key on a route or a stream is a compile error", () => {
  const route = check(
    `${preamble}export const r = http.get("/a", { response: Out, resources: [db], nonsense: 1 }, async () => ({ n: 1 }));`,
  );
  assert.match(route, /never/, route);
  const stream = check(
    `${preamble}export const s = http.stream("/s", { resources: [db], nonsense: 1 }, async () => {});`,
  );
  assert.match(stream, /never/, stream);
});

test("the declarations the runtime accepts still compile, and stay typed", () => {
  const out = check(`${preamble}export const r = http.get(
  "/c",
  { response: Out, auth: session, resources: [db], summary: "ok" },
  async (ctx) => {
    const id: string = ctx.auth.userId;
    await ctx.resources.main.query("select 1");
    return { n: id.length };
  },
);
export const s = http.stream("/s", { resources: [db] }, async (ctx, stream) => {
  await ctx.resources.main.query("select 1");
  stream.event("tick", { n: 1 });
});`);
  assert.equal(out, "", out);
});

test("a task's return type reaches ctx.tasks.invoke", () => {
  // `invoke` used to resolve `unknown`, so the first thing a real service
  // wrote after following GUIDE §5 was `error TS18046: 't' is of type
  // 'unknown'` (found by an operator building a service to test an upgrade).
  // `task()` returns a workload that remembers its handler's output.
  const out = check(`
    import { defineApp, http, task } from "@sakaladev/usai";
    import { z } from "zod";
    const count = task("count", { input: z.object({ n: z.number() }) }, async (ctx) => ({
      doubled: ctx.input.n * 2,
    }));
    export const run = http.get("/run", { response: z.object({ doubled: z.number() }) }, async (ctx) => {
      const result = await ctx.tasks.invoke(count, { n: 21 });
      return { doubled: result.doubled };
    });
    export default defineApp({ name: "typed", workloads: [count, run] });
  `);
  assert.equal(out, "", `invoke should be typed from the task:\n${out}`);
});

test("a workload list still accepts every kind of workload", () => {
  // TypedWorkload must stay a Workload everywhere one is expected.
  const out = check(`
    import { defineApp, http, task, cron } from "@sakaladev/usai";
    import { z } from "zod";
    const t = task("t", {}, async () => 1);
    const c = cron("c", { schedule: "* * * * *" }, async () => {});
    const r = http.get("/", { response: z.object({}) }, async () => ({}));
    export default defineApp({ name: "mixed", workloads: [t, c, r] });
  `);
  assert.equal(out, "", out);
});

test("a body bound on a kind that has no body is a compile error", () => {
  // `maxBodyBytes` sat on the policies every kind extended, so a task, a
  // cron, a command, a stream or a socket could declare one, ship it in the
  // manifest, and have the runtime refuse the whole application at install
  // ("a body bound on a workload that has no body"). The compiler now says
  // what the runtime says, at the line that wrote it.
  for (const source of [
    `import { task } from "@sakaladev/usai";
     export const t = task("t", { maxBodyBytes: 1024 }, async () => 1);`,
    `import { cron } from "@sakaladev/usai";
     export const c = cron("c", { schedule: "* * * * *", maxBodyBytes: 1024 }, async () => {});`,
    `import { command } from "@sakaladev/usai";
     export const c = command("c", { maxBodyBytes: 1024 }, async () => {});`,
    `import { http } from "@sakaladev/usai";
     export const s = http.stream("/s", { maxBodyBytes: 1024 }, async () => {});`,
    // `socket` is its own export: written as `http.socket` this case passed
    // with `maxBodyBytes` deleted, because the member does not exist at all.
    `import { socket } from "@sakaladev/usai";
     export const s = socket("/s", { maxBodyBytes: 1024 }, { open: async () => {} });`,
  ]) {
    assert.notEqual(check(source), "", `maxBodyBytes should not compile here:\n${source}`);
  }
  // The kind that does have a body still takes it.
  assert.equal(
    check(
      `${preamble}export const r = http.post("/a", { response: Out, maxBodyBytes: 1024 }, async () => ({ n: 1 }));`,
    ),
    "",
  );
});

test("a consumer's only admission policy is its deadline", () => {
  // `concurrency:` on a consumer is the topic's — how many messages that
  // consumer takes at once — and it used to arrive twice: once from the
  // consumer's own option, once from the policy set every kind extended,
  // with the same name and a different meaning. A consumer keeps the topic's
  // one and takes no body bound.
  assert.notEqual(
    check(`import { queue } from "@sakaladev/usai";
     export const c = queue.consume("t", { maxBodyBytes: 1024 }, async () => {});`),
    "",
  );
  assert.equal(
    check(`import { queue } from "@sakaladev/usai";
     export const c = queue.consume("t", { timeout: "30s", concurrency: 4 }, async () => {});`),
    "",
  );
});

test("documenting one status's response headers needs no other entry", () => {
  // `Record<number | "*", …>` made `"*"` a required key, so a route that
  // documented only its `201: location` header did not compile.
  assert.equal(
    check(
      `${preamble}export const r = http.post(
         "/a",
         { response: { 201: Out }, responseHeaders: { 201: { location: "URL of the new one" } } },
         async () => ({ n: 1 }) as never,
       );`,
    ),
    "",
  );
});

test("the env cast the guide prints is the one that compiles", () => {
  // GUIDE §Environment and `ctx.env`'s own doc comment both told readers to
  // write `ctx.env as EnvValues<typeof spec>`. `spec` is the declaration,
  // and `EnvValues` took the field map inside it, so the documented line
  // was a compile error for everyone who copied it.
  const out = check(`
    import { env, http, type EnvValues } from "@sakaladev/usai";
    import { z } from "zod";
    const spec = env({ WORKERS: env.int(), MODE: env.enum(["a", "b"]), NOTE: env.optional(env.string()) });
    export const r = http.get("/e", { response: z.object({ n: z.number() }) }, async (ctx) => {
      const e = ctx.env as EnvValues<typeof spec>;
      const workers: number = e.WORKERS;
      const mode: "a" | "b" = e.MODE;
      const note: string | undefined = e.NOTE;
      return { n: workers + mode.length + (note?.length ?? 0) };
    });
  `);
  assert.equal(out, "", out);
});

test("a raw endpoint's principal is on its context", () => {
  // `http.raw` accepted `auth:`, ran the resolver, refused with 401 — and
  // `RawContext` had no `auth`, so the principal a signed-webhook endpoint
  // had just proved could only be reached through a cast.
  const out = check(`
    import { http, auth, postgres } from "@sakaladev/usai";
    const db = postgres("main", {});
    const tenant = auth.header({
      name: "tenant",
      header: "x-signature",
      resources: [db],
      resolve: async (ctx) => {
        await ctx.resources.main.query("select 1");
        return { tenantId: "t" };
      },
    });
    export const hook = http.raw("/hook", { auth: tenant, resources: [db] }, async (ctx) => {
      const id: string = ctx.auth.tenantId;
      await ctx.resources.main.query("select 1", [id]);
      return { status: 204, headers: {}, body: new Uint8Array(0) } as never;
    });
  `);
  assert.equal(out, "", out);
});

test("the graph annotations do not erase a task's return type", () => {
  // `dispatches` and `publishes` are annotations that return `from` — and
  // they returned it as a bare `Workload`, so wrapping a task in either one
  // (the idiom the guide prints) took `ctx.tasks.invoke` back to `unknown`.
  const out = check(`
    import { defineApp, http, task, dispatches, publishes } from "@sakaladev/usai";
    import { z } from "zod";
    const inner = task("inner", {}, async () => ({ ok: true }));
    const count = publishes(
      dispatches(task("count", { input: z.object({ n: z.number() }) }, async (ctx) => ({
        doubled: ctx.input.n * 2,
      })), inner),
      "audit",
    );
    export const run = http.get("/run", { response: z.object({ doubled: z.number() }) }, async (ctx) => {
      const result = await ctx.tasks.invoke(count, { n: 21 });
      return { doubled: result.doubled };
    });
    export default defineApp({ name: "annotated", workloads: [inner, count, run] });
  `);
  assert.equal(out, "", out);
});

test("ctx.log offers every level the guest emits", () => {
  // `ctx.log` *is* `console` inside a world, and the guest's `console.log`
  // emits at `info` — but the type omitted `log`, so the first line most
  // people write did not compile against the object that implements it.
  const out = check(`${preamble}export const r = http.get("/l", { response: Out }, async (ctx) => {
    ctx.log.log("starting", { step: 1 });
    ctx.log.debug("d");
    ctx.log.info("i");
    ctx.log.warn("w");
    ctx.log.error("e");
    return { n: 1 };
  });`);
  assert.equal(out, "", out);
});

test("a task's input is typed at the call site, not only at the first run", () => {
  // `invoke<W extends Workload>(task: W, input?: unknown)` typed the output
  // precisely and the input not at all: calling a task that declares an
  // `input` schema with **no input**, or with `42`, compiled. The runtime's
  // refusal is good — `input failed validation` with the failing pointers —
  // but it arrives at the first run, and for a module's task that is the
  // half of its surface a consumer most wants held.
  const ok = check(`
    import { defineApp, http, task } from "@sakaladev/usai";
    import { z } from "zod";
    const issue = task("issue", { input: z.object({ customerId: z.string() }) }, async (ctx) => ({
      id: ctx.input.customerId,
    }));
    export const r = http.get("/r", { response: z.object({ id: z.string() }) }, async (ctx) => {
      return await ctx.tasks.invoke(issue, { customerId: "c1" });
    });
    export default defineApp({ name: "typed-input", workloads: [issue, r] });
  `);
  assert.equal(ok, "", ok);
  for (const call of [
    "await ctx.tasks.invoke(issue);",
    "await ctx.tasks.invoke(issue, 42);",
    "await ctx.tasks.dispatch(issue);",
  ]) {
    const out = check(`
      import { http, task } from "@sakaladev/usai";
      import { z } from "zod";
      const issue = task("issue", { input: z.object({ customerId: z.string() }) }, async (ctx) => ({
        id: ctx.input.customerId,
      }));
      export const r = http.get("/r", { response: z.object({}) }, async (ctx) => {
        ${call}
        return {};
      });
    `);
    assert.notEqual(out, "", `should not compile: ${call}`);
  }
  // A task with no declared input keeps the permissive shape.
  const bare = check(`
    import { http, task } from "@sakaladev/usai";
    import { z } from "zod";
    const sweep = task("sweep", {}, async () => 1);
    export const r = http.get("/r", { response: z.object({}) }, async (ctx) => {
      await ctx.tasks.invoke(sweep);
      await ctx.tasks.dispatch(sweep, { anything: true });
      return {};
    });
  `);
  assert.equal(bare, "", bare);
});
