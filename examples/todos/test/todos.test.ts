import { test } from "node:test";
import assert from "node:assert/strict";
import { testApp } from "@sakaladev/usai/test";

// Needs a database: DATABASE_URL (a throwaway one — the test migrates it).
const url = process.env["DATABASE_URL"];

test("todos: create, complete, list, activity via task, cron and command", {
  skip: url ? false : "set DATABASE_URL",
}, async () => {
  const root = new URL("..", import.meta.url).pathname;
  const app = await testApp({
    root,
    env: { DATABASE_URL: url!, APP_ENV: "development" },
    migrate: { seed: true },
  });
  try {
    const created = await app.http.post("/todos", { body: { title: "write the tutorial" } });
    assert.equal(created.status, 201, created.text);
    const todo = created.body as { id: number; done: boolean };
    assert.equal(todo.done, false);

    const bad = await app.http.post("/todos", { body: { title: "" } });
    assert.equal(bad.status, 400, "boundary validation happens before any world exists");

    const done = await app.http.post(`/todos/${todo.id}/complete`);
    assert.equal(done.status, 200, done.text);
    const again = await app.http.post(`/todos/${todo.id}/complete`);
    assert.equal(again.status, 409);

    const open = await app.http.get("/todos", { query: { done: "false" } });
    assert.equal(open.status, 200);
    assert.ok(!(open.body as Array<{ id: number }>).some((t) => t.id === todo.id));

    // The activity rows are written by dispatched tasks; invoke the task
    // directly to see it work, and the command to read the totals.
    const recorded = await app
      .task("record-activity")
      .invoke({ todoId: todo.id, event: "deleted" });
    assert.equal(recorded.ok, true, JSON.stringify(recorded.error));
    const purged = await app.cron("purge-completed").run<{ purged: number }>();
    assert.equal(purged.ok, true);
    const stats = await app.command("stats").run<{ total: number; done: number }>();
    assert.equal(stats.ok, true);
    assert.ok(stats.value.total >= 4 && stats.value.done >= 1, JSON.stringify(stats.value)); // 3 seeded + ours

    const gone = await app.http.delete(`/todos/${todo.id}`);
    assert.equal(gone.status, 204);
    assert.equal((await app.http.get(`/todos/${todo.id}`)).status, 404);
  } finally {
    await app.close();
  }
});
