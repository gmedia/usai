import { defineModule, http, type PostgresHandle } from "@sakaladev/usai";
import { z } from "zod";
import { db } from "../resources.ts";

export const listUsers = http.get("/users", { response: { 200: z.array(z.object({ id: z.number(), name: z.string() })) }, resources: [db] }, async (ctx) =>
  (ctx.resources["main"] as PostgresHandle).query<{ id: number; name: string }>(`select id, name from users order by id`),
);

export const users = defineModule({
  name: "users",
  workloads: [listUsers],
  resources: [db],
  migrations: "./src/users/migrations/*.sql",
  seeders: "./src/users/seeders/*.ts",
});
