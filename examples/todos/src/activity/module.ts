import { defineModule, task, type PostgresHandle } from "@sakaladev/usai";
import { z } from "zod";
import { db } from "../resources.ts";

// A task owns a fresh world: dispatched from an HTTP world, it runs after
// that request has already answered (ownership transferred, ADR-0010).
export const record = task("record-activity", { input: z.object({ todoId: z.number().int(), event: z.enum(["created", "completed", "deleted"]) }), resources: [db] }, async (ctx) => {
  const sql = ctx.resources["main"] as PostgresHandle;
  await sql.execute(`insert into activity (todo_id, event) values ($1, $2)`, [ctx.input.todoId, ctx.input.event]);
  return { recorded: ctx.input.event };
});

export const activity = defineModule({
  name: "activity",
  workloads: [record],
  resources: [db],
  migrations: "./src/activity/migrations/*.sql",
});
