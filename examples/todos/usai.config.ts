import { defineConfig } from "usai/config";

export default defineConfig({
  app: "./src/app.ts",
  database: {
    // Colocated with each module; `usai db migrate` applies them in file-name order.
    migrations: { include: ["./src/**/migrations/*.sql"] },
    seeders: { include: ["./src/**/seeders/*.ts"] },
  },
});
