/**
 * `@sakaladev/usai/config` — the shape of `usai.config.ts`: project
 * structure only (entry, output directory, migration and seeder globs),
 * declarative. Start with {@link defineConfig}.
 *
 * @module
 */

/** Where the SQL migrations and seeders live, as globs from the project root.
 *
 * @category Configuration
 */
export interface DatabaseConfig {
  migrations?: { include: string[] };
  seeders?: { include: string[] };
}

/** `usai.config.ts`: project structure only (entry, output, database
 * globs). Declarative — the file is evaluated in a capability-less world,
 * so it cannot read the environment or the file system.
 *
 * @category Configuration
 */
export interface UsaiConfig {
  /** Application entry. Default `./src/app.ts`. */
  app?: string;
  /** Build output directory. Default `.usai/build`. */
  outDir?: string;
  database?: DatabaseConfig;
}

/**
 * The default export of `usai.config.ts`.
 *
 * @example
 * ```ts
 * import { defineConfig } from "@sakaladev/usai/config";
 * export default defineConfig({ app: "./src/app.ts", database: { migrations: { include: ["./migrations/*.sql"] } } });
 * ```
 *
 * @category Configuration
 */
export function defineConfig(config: UsaiConfig): UsaiConfig & { readonly __usai: "config" } {
  return { __usai: "config", ...config };
}
