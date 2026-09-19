[@sakaladev/usai](../../README.md) / [test](../README.md) / testApp

# Function: testApp()

```ts
function testApp(options?: TestAppOptions): Promise<TestApp>;
```

Start the application under the real runtime for a test: the same
artifact, the same boundaries, the same worlds production will run.
The harness spawns the `usai` binary (`USAI_BIN`, `binary`, or `usai` on
PATH) on random ports with a control token, waits for it to announce
itself, and talks HTTP to the application and to the control surface.
Tasks, cron ticks and commands run deterministically through the
control surface — no wall clock, no external server — and answer with a
[WorkOutcome](../interfaces/WorkOutcome.md) that includes the world's lifecycle violations, so a
test can fail on detached work the way production would.

`usai test` sets `USAI_BIN` and runs `node --test` over the project.

## Parameters

| Parameter | Type |
| ------ | ------ |
| `options` | [`TestAppOptions`](../interfaces/TestAppOptions.md) |

## Returns

`Promise`\<[`TestApp`](../interfaces/TestApp.md)\>

## Example

```ts
import { test, before, after } from "node:test";
import assert from "node:assert/strict";
import { testApp, type TestApp } from "@sakaladev/usai/test";

let app: TestApp;
before(async () => { app = await testApp({ root: ".", migrate: true }); });
after(() => app.close());

test("creates a user and hands off the welcome mail", async () => {
  const res = await app.http.post("/users", { body: { name: "Ayu" } });
  assert.equal(res.status, 201);
  const mail = await app.task("send-welcome").invoke({ userId: (res.body as { id: string }).id });
  assert.ok(mail.ok, mail.error?.message);
  assert.deepEqual(mail.violations, []);
});
```
