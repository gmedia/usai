import { test } from "node:test";
import assert from "node:assert/strict";
import { testApp } from "@sakaladev/usai/test";

// `pnpm test` (= `usai test`) runs this through the real runtime: the app is
// built once, each request gets a fresh world, nothing is mocked.
test("hello greets and rejects", async () => {
  const app = await testApp({ root: new URL("..", import.meta.url).pathname });
  try {
    const ok = await app.http.get("/hello/world");
    assert.equal(ok.status, 200);
    assert.deepEqual(ok.body, { hello: "world" });
    const nobody = await app.http.get("/hello/nobody");
    assert.equal(nobody.status, 404);
  } finally {
    await app.close();
  }
});
