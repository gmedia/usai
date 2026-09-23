import { defineApp, http, errors } from "@sakaladev/usai";
import { z } from "zod";

const Params = z.object({ name: z.string().min(1).max(40) });
const Greeting = z.object({ hello: z.string() });

export const hello = http.get(
  "/hello/:name",
  { params: Params, response: Greeting },
  async (ctx) => {
    if (ctx.params.name === "nobody") throw errors.notFound("nobody is not here");
    return { hello: ctx.params.name };
  },
);

// A database, when you want one: uncomment these three blocks, put
// DATABASE_URL in .env (compose.yaml has a postgres service to uncomment
// too), and run `pnpm usai db migrate` — migrations/0001_notes.sql is
// already here and creates the table this route reads.
//
// (add `postgres` to the import at the top of this file)
//
// const db = postgres("db");
//
// const Note = z.object({ id: z.number().int(), body: z.string() });
//
// export const notes = http.get(
//   "/notes",
//   { response: z.array(Note), resources: [db] },
//   async (ctx) =>
//     ctx.resources.db.query<z.infer<typeof Note>>(
//       "select id, body from notes order by id desc limit 50",
//     ),
// );

export default defineApp({
  name: "__NAME__",
  workloads: [hello],
  // resources: [db],
  // workloads: [hello, notes],
});
