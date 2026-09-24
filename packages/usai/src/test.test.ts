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
    // The half that matters: a dispatched task that *fails* leaves nothing
    // of the application's behind, so the runtime's own line is the only
    // record — and it carried neither the request id nor the workload, so
    // the documented idiom (`logs({ requestId })`, "including the tasks it
    // dispatched") found nothing at all for the failure case. `task=` is the
    // dispatch's own id (`task:always-throws#d1`), which `workload` does not
    // match.
    const failing = await app.http.get("/request-id/hand-off-failing", {
      headers: { "x-request-id": "ticket-43" },
    });
    assert.equal(failing.status, 200, failing.text);
    const failure = await app.waitForLog({
      requestId: "ticket-43",
      workload: "task:always-throws",
      message: "task failed",
    });
    assert.equal(failure.level, "WARN");
    assert.match(failure.message, /task failed/);
    // And the 5xx line an on-call clicks through to from a dashboard: it is
    // the pivot to "everything else that happened for this request", and it
    // carried no request id at all, so the only join was `world` — unique
    // per process, and named in no document as a join key.
    const boom = await app.http.get("/boom", { headers: { "x-request-id": "ticket-44" } });
    assert.equal(boom.status, 500, boom.text);
    const served = await app.waitForLog({
      requestId: "ticket-44",
      where: (l) => l.target !== "app" && l.level === "ERROR",
    });
    assert.match(served.message, /application error|unexpected handler failure/);
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
    // The schedulers are off by default, so a suite waiting for a line that
    // only a consumer writes fails with "0 lines seen" and no clue that the
    // thing that writes it was never started. Without the release note in
    // front of you, that is an hour.
    await assert.rejects(app.waitForLog({ message: "message dead-lettered" }, 200), (e: Error) => {
      assert.match(e.message, /schedulers: \{ queue: true \}/, e.message);
      return true;
    });
    // And it stays quiet when the schedulers have nothing to do with it.
    await assert.rejects(app.waitForLog({ message: "never written" }, 200), (e: Error) => {
      assert.doesNotMatch(e.message, /schedulers/, e.message);
      return true;
    });
  } finally {
    await app.close();
  }
});

test("the harness opens event streams and WebSockets the way a browser would", {
  skip: !existsSync(binary) ? "usai binary not built" : false,
}, async () => {
  const { testApp, TestSocketRefused } = await import("./test.ts");
  const app = await testApp({
    root: fixture,
    binary,
    env: { UPSTREAM_URL: "http://127.0.0.1:9/" },
  });
  try {
    // SSE: events one by one, then the end of the stream.
    const events = await app.stream("/events", { query: { n: 2 } });
    assert.equal(events.status, 200);
    assert.equal(events.headers["content-type"], "text/event-stream");
    assert.equal(events.headers["x-stream"], "yes");
    assert.deepEqual(await events.next(), { event: "tick", data: '{"i":1}', json: { i: 1 } });
    assert.deepEqual((await events.next())?.json, { i: 2 });
    assert.deepEqual((await events.next())?.json, { total: 2 });
    assert.equal(await events.next(), null, "the handler returned: the stream ended");
    // Leaving early is allowed and is not a failed stream.
    const endless = await app.stream("/endless");
    assert.equal(endless.status, 200);
    endless.close();
    // A non-SSE stream reads as text.
    const csv = await app.stream("/export.csv");
    assert.equal(csv.headers["content-type"], "text/csv");
    assert.equal(await csv.text(), "id,name\n1,Ayu\n");
    // WebSocket: JSON both ways, the server's close frame observed.
    const ws = await app.socket("/chat", {});
    await ws.send({ text: "hello" });
    assert.deepEqual(await ws.next(), { echo: "anon: hello", count: 1 });
    await ws.send({ text: "bye" });
    assert.deepEqual(await ws.next(), { echo: "anon: bye", count: 2 });
    const closed = await ws.closed();
    assert.equal(closed.code, 1000, JSON.stringify(closed));
    assert.equal(closed.reason, "bye then");
    // A bearer token rides as the second subprotocol; a refused one is a 401 before any frame.
    const authed = await app.socket("/private-chat", { protocols: ["bearer", "secret"] });
    assert.equal(authed.protocol, "bearer");
    assert.deepEqual(await authed.next(), { user: "u1" });
    await authed.close();
    await assert.rejects(
      app.socket("/private-chat", { protocols: ["bearer", "wrong"] }),
      (e: unknown) => e instanceof TestSocketRefused && e.status === 401,
    );
    assert.deepEqual(
      app.logs({ message: "stream handler failed" }),
      [],
      "a client leaving is not a failure",
    );
    // A declared event that does not match its schema ends the stream early
    // and is a failed stream with the event named in the log.
    const bad = await app.stream("/bad-event");
    assert.deepEqual((await bad.next())?.json, { i: 1 });
    assert.equal(await bad.next(), null, "the stream ended at the violation");
    const failure = await app.waitForLog({ message: "stream handler failed" });
    assert.match(String(failure["error"]), /event_contract_violation|event tick does not match/);
  } finally {
    await app.close();
  }
});
