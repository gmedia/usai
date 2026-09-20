import { defineConfig } from "@sakaladev/usai/config";

export default defineConfig({
  app: "./src/app.ts",
  database: { migrations: { include: ["./migrations/*.sql"] } },
});
