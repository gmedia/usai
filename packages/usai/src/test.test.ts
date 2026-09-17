// Exercises `usai/test` against examples/hello with the repository's own
// binary. Skips when the binary has not been built.
import { test } from "node:test";
import assert from "node:assert/strict";
import { existsSync } from "node:fs";
import { resolve } from "node:path";

const binary = process.env["USAI_BIN"] ?? resolve(import.meta.dirname, "../../../target/debug/usai");
const root = resolve(import.meta.dirname, "../../../examples/hello");

test("usai/test drives the application through the real runtime", { skip: !existsSync(binary) ? "usai binary not built" : false }, async () => {
  const { testApp } = await import("./test.ts");
  const app = await testApp({ root, binary });
  try {
    const ok = await app.http.get("/hello/world");
    assert.equal(ok.status, 200);
    assert.deepEqual(ok.body, { hello: "world" });
    const missing = await app.http.get("/hello/nobody");
    assert.equal(missing.status, 404);
    assert.equal((missing.body as { error: { code: string } }).error.code, "not_found");
    const invalid = await app.http.get("/hello/" + "x".repeat(50));
    assert.equal(invalid.status, 400);
    const status = await app.status();
    assert.ok(status["engine"] === "wasm" || status["engine"] === "quickjs");
    await assert.rejects(app.task("does-not-exist").invoke(), /unknown workload/);
  } finally {
    await app.close();
  }
});
