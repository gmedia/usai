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
