// A realistic Usai application — the tutorial in docs/GUIDE.md walks it.
//
//   export DATABASE_URL=postgres://user:pass@localhost:5432/todos
//   usai db migrate --root examples/todos
//   usai db seed --root examples/todos
//   usai dev --root examples/todos
//   curl -X POST localhost:3000/todos -H 'content-type: application/json' -d '{"title":"read the guide"}'
//   curl 'localhost:3000/todos?done=false'
//   usai app stats --root examples/todos
//   usai test --root examples/todos
import { defineApp, env } from "@sakaladev/usai";
import { todos } from "./todos/module.ts";
import { activity } from "./activity/module.ts";

export default defineApp({
  name: "todos",
  modules: [todos, activity],
  env: env({
    DATABASE_URL: env.url(),
    APP_ENV: env.optional(env.enum(["development", "production"])),
  }),
});
