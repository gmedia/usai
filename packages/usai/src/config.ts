// `usai.config.ts` — maps project structure into the build model
// (`GOAL.md` §26–§31, contract C9). Declarative only.

export interface DatabaseConfig {
  migrations?: { include: string[] };
  seeders?: { include: string[] };
}

export interface UsaiConfig {
  /** Application entry. Default `./src/app.ts`. */
  app?: string;
  /** Build output directory. Default `.usai/build`. */
  outDir?: string;
  database?: DatabaseConfig;
}

export function defineConfig(config: UsaiConfig): UsaiConfig & { readonly __usai: "config" } {
  return { __usai: "config", ...config };
}
