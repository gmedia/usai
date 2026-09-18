// Resource declarations (`GOAL.md` §23–§25, contract C18). Each is a distinct
// persistence class with a stated contract; there is no generic
// `persistent(() => …)`.

import type { ResourceDeclaration } from "./declarations.ts";

export interface CacheLocalOptions {
  maxEntries?: number;
}

/** Shared across worlds, local to the runtime, not durable, may disappear on
 * restart. */
export interface CacheLocalDeclaration extends ResourceDeclaration {
  readonly kind: "cache.local";
}

export const cache = {
  local(name: string, options: CacheLocalOptions = {}): CacheLocalDeclaration {
    return {
      __usai: "resource",
      name,
      kind: "cache.local",
      config: options.maxEntries === undefined ? {} : { maxEntries: options.maxEntries },
      env: [],
      methods: ["get", "set", "delete", "increment", "clear"],
    };
  },
};

/** The in-world handle for a `cache.local` resource. */
export interface CacheLocalHandle {
  get<T = unknown>(key: string): Promise<T | null>;
  set(key: string, value: unknown, options?: { ttlMs?: number }): Promise<boolean>;
  delete(key: string): Promise<boolean>;
  increment(key: string, by?: number): Promise<number>;
  clear(): Promise<boolean>;
}

export interface PostgresOptions {
  /** Environment variable holding the connection URL. Default `DATABASE_URL`. */
  urlEnv?: string;
  pool?: {
    max?: number;
    /** `clean` (default) resets session state on every checkout; `fast`
     * skips that round-trip for applications that never touch session
     * state. */
    recycling?: "clean" | "fast";
  };
  /** TLS is chosen by the URL's `sslmode` (`disable` | `prefer` | `require`)
   * and the server certificate is always verified — against Mozilla's roots
   * plus this PEM bundle (private CAs, managed-database roots). When unset,
   * the runtime also honours `PGSSLROOTCERT` in the environment. */
  tls?: { caFile?: string };
}

/** A PostgreSQL pool owned by the runtime. Each operation leases one
 * connection; reuse follows terminal proof (contract C5). */
export interface PostgresDeclaration extends ResourceDeclaration {
  readonly kind: "postgres";
}

export function postgres(name: string, options: PostgresOptions = {}): PostgresDeclaration {
  const urlEnv = options.urlEnv ?? "DATABASE_URL";
  const config: Record<string, unknown> = { urlEnv };
  const pool: Record<string, unknown> = {};
  if (options.pool?.max !== undefined) pool["max"] = options.pool.max;
  if (options.pool?.recycling !== undefined) pool["recycling"] = options.pool.recycling;
  if (Object.keys(pool).length > 0) config["pool"] = pool;
  if (options.tls?.caFile !== undefined) config["tls"] = { caFile: options.tls.caFile };
  return {
    __usai: "resource",
    name,
    kind: "postgres",
    config,
    env: [urlEnv],
    methods: ["query", "one", "execute"],
  };
}

export type SqlParam = string | number | boolean | null | Record<string, unknown> | unknown[];

/** The in-world handle for a `postgres` resource. Rows are plain objects
 * keyed by column name; values are JSON (uuid/timestamps as strings). */
export interface PostgresHandle {
  query<T = Record<string, unknown>>(sql: string, params?: SqlParam[]): Promise<T[]>;
  one<T = Record<string, unknown>>(sql: string, params?: SqlParam[]): Promise<T | null>;
  execute(sql: string, params?: SqlParam[]): Promise<number>;
}
