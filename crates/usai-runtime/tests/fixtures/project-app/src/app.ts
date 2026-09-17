import { defineApp, env, http } from "usai";
import { users } from "./users/module.ts";
import { billing } from "./billing/module.ts";

export const config = http.get("/config", {}, async (ctx) => ({ env: ctx.env }));

export default defineApp({
  name: "project-fixture",
  modules: [users, billing],
  workloads: [config],
  env: env({
    DATABASE_URL: env.url(),
    APP_ENV: env.enum(["development", "production"]),
    WORKERS: env.int(),
    DEBUG: env.optional(env.bool()),
  }),
});
