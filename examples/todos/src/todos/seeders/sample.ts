import { seeder, type PostgresHandle } from "@sakaladev/usai";
import { db } from "../../resources.ts";

// `usai db seed sample` — runs in its own finite world with the pool leased.
export default seeder({ resources: [db] }, async (ctx) => {
  const sql = ctx.resources["main"] as PostgresHandle;
  await sql.execute(`insert into todos (title) values ($1), ($2), ($3)`, ["write the guide", "measure before optimizing", "ship the alpha"]);
});
