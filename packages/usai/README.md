# usai

The developer SDK for [Usai](https://github.com/gmedia/usai), a lifecycle-native
application runtime: you declare workloads (HTTP endpoints, tasks, cron,
commands, queue consumers, WebSockets, streams, services) and resources
(PostgreSQL, runtime-local cache, configuration); the runtime gives each unit
of work a fresh execution world with exactly the lifetime its kind implies.

```ts
import { defineApp, http } from "@sakaladev/usai";
import { z } from "zod";

export const hello = http.get("/hello/:name", { params: z.object({ name: z.string().min(1) }) }, async (ctx) => ({
  hello: ctx.params.name,
}));

export default defineApp({ name: "hello", workloads: [hello] });
```

```bash
pnpm dlx @sakaladev/create-usai my-app && cd my-app && pnpm install
usai dev            # the usai binary: https://github.com/gmedia/usai/releases
```

- Guide: https://github.com/gmedia/usai/blob/main/docs/GUIDE.md
- Test harness: `import { testApp } from "@sakaladev/usai/test"` drives the real runtime.
- Status: developer preview (`0.0.x`). The API will change before the first stable release.

Apache-2.0.
