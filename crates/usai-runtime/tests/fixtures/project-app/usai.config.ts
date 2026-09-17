import { defineConfig } from "usai/config";

// Both layouts at once: colocated per module and a centralized directory.
export default defineConfig({
  app: "./src/app.ts",
  database: {
    migrations: { include: ["./src/**/migrations/*.sql", "./migrations/*.sql"] },
    seeders: { include: ["./src/**/seeders/*.ts", "./seeders/*.ts"] },
  },
});
