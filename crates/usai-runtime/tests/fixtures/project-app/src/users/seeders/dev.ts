import { seeder, type PostgresHandle } from "@sakaladev/usai";
import { db } from "../../resources.ts";

export default seeder({ resources: [db] }, async (ctx) => {
  const sql = ctx.resources["main"] as PostgresHandle;
  await sql.execute(`insert into users (name) values ($1), ($2)`, ["Ayu", "Budi"]);
  console.log("seeded users");
});
