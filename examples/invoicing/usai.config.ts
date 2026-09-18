import { defineConfig } from "@sakaladev/usai/config";

export default defineConfig({
  app: "./src/app.ts",
  database: {
    migrations: { include: ["./src/**/migrations/*.sql"] },
    seeders: { include: ["./src/**/seeders/*.ts"] },
  },
});
