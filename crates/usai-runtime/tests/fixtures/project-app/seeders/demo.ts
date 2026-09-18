import { seeder, type PostgresHandle } from "@sakaladev/usai";
import { db } from "../src/resources.ts";

export default seeder({ resources: [db] }, async (ctx) => {
  const sql = ctx.resources["main"] as PostgresHandle;
  await sql.execute(`insert into invoices (user_id, amount) select id, 10.5 from users`);
});
