// Exercises `usai/test` against examples/hello with the repository's own
// binary. Skips when the binary has not been built.
import { test } from "node:test";
import assert from "node:assert/strict";
import { existsSync } from "node:fs";
import { resolve } from "node:path";

const binary =
  process.env["USAI_BIN"] ?? resolve(import.meta.dirname, "../../../target/debug/usai");
const root = resolve(import.meta.dirname, "../../../examples/hello");

test("usai/test drives the application through the real runtime", {
  skip: !existsSync(binary) ? "usai binary not built" : false,
}, async () => {
  const { testApp } = await import("./test.ts");
  const app = await testApp({ root, binary });
  try {
    const ok = await app.http.get("/hello/world");
    assert.equal(new TextDecoder().decode(ok.bytes), ok.text, "bytes are the exact body");
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

const fixture = resolve(
  import.meta.dirname,
  "../../../crates/usai-runtime/tests/fixtures/http-app",
);

test("the harness keeps the runtime's log and joins a request with the tasks it handed off", {
  skip: !existsSync(binary) ? "usai binary not built" : false,
}, async () => {
  const { testApp } = await import("./test.ts");
  const app = await testApp({
    root: fixture,
    binary,
    env: { UPSTREAM_URL: "http://127.0.0.1:9/" },
  });
  try {
    const res = await app.http.get("/request-id/hand-off", {
      headers: { "x-request-id": "support-ticket-42" },
    });
    assert.equal(res.status, 200, res.text);
    assert.equal(res.headers["x-request-id"], "support-ticket-42");
    // The dispatched task runs after the response: wait for its line.
    const line = await app.waitForLog({
      requestId: "support-ticket-42",
      workload: "task:echo-request-id",
      message: "echoed request id",
    });
    assert.equal(line.level, "INFO");
    assert.equal(line.target, "app");
    assert.deepEqual(line.fields, { requestId: "support-ticket-42" });
    // Both hand-offs — the invoked child and the dispatched one — carry the id
    // (two worlds; the second line may land after the first was seen).
    await app.waitForLog({
      requestId: "support-ticket-42",
      where: (l) => l.world !== line.world,
    });
    const forRequest = app.logs({ requestId: "support-ticket-42", target: "app" });
    assert.equal(forRequest.length, 2, JSON.stringify(forRequest));
    assert.deepEqual(app.logs({ requestId: "someone-else" }), []);
    await assert.rejects(
      app.waitForLog({ message: "never written" }, 200),
      /no log line matched message=never written within 200 ms/,
    );
  } finally {
    await app.close();
  }
});
