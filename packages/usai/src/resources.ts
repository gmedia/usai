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

/** The statements available on one connection. */
export interface SqlExecutor {
  query<T = Record<string, unknown>>(sql: string, params?: SqlParam[]): Promise<T[]>;
  one<T = Record<string, unknown>>(sql: string, params?: SqlParam[]): Promise<T | null>;
  execute(sql: string, params?: SqlParam[]): Promise<number>;
}

/** The in-world handle for a `postgres` resource. Rows are plain objects
 * keyed by column name; values are JSON (uuid/timestamps as strings). Each
 * statement leases its own connection; `transaction` pins one for the
 * callback and commits when it returns, rolls back when it throws. A world
 * that ends with the transaction still open is a lifecycle error, and the
 * runtime rolls back on its behalf. */
export interface PostgresHandle extends SqlExecutor {
  transaction<T>(fn: (tx: SqlExecutor) => Promise<T>): Promise<T>;
}

export interface HttpClientOptions {
  /** Every request is relative to it; another origin is refused. Without
   * it the client may call any http(s) URL. */
  baseUrl?: string;
  /** Environment variable holding the base URL (staging and production
   * differ; the declaration does not). */
  baseUrlEnv?: string;
  /** Per-request timeout in milliseconds (default 10 000). */
  timeoutMs?: number;
  /** In-flight bound; the next request is refused with 503, not queued. */
  maxConcurrent?: number;
  /** Static headers on every request. */
  headers?: Record<string, string>;
  /** Environment variable whose value is sent as `Authorization: Bearer …`. */
  bearerTokenEnv?: string;
}

/** Outbound HTTP, declared: the runtime owns the client (pool, TLS roots,
 * timeouts), every request is an operation owned by the world, and the
 * destination is visible in `usai graph` and the API docs. There is no
 * global `fetch` inside a world. */
export interface HttpClientDeclaration extends ResourceDeclaration {
  readonly kind: "http.client";
}

export function httpClient(name: string, options: HttpClientOptions = {}): HttpClientDeclaration {
  const config: Record<string, unknown> = {};
  if (options.baseUrl !== undefined) config["baseUrl"] = options.baseUrl;
  if (options.baseUrlEnv !== undefined) config["baseUrlEnv"] = options.baseUrlEnv;
  if (options.timeoutMs !== undefined) config["timeoutMs"] = options.timeoutMs;
  if (options.maxConcurrent !== undefined) config["maxConcurrent"] = options.maxConcurrent;
  if (options.headers !== undefined) config["headers"] = options.headers;
  if (options.bearerTokenEnv !== undefined) config["bearerTokenEnv"] = options.bearerTokenEnv;
  return {
    __usai: "resource",
    name,
    kind: "http.client",
    config,
    env: [options.baseUrlEnv, options.bearerTokenEnv].filter((v): v is string => v !== undefined),
    methods: ["fetch"],
  };
}

export interface FetchInit {
  method?: string;
  headers?: Record<string, string>;
  /** A string body, or a JSON value (serialized, `content-type: application/json`). */
  body?: string;
  json?: unknown;
  /** Lower than the resource's timeout only. */
  timeoutMs?: number;
}

export interface FetchResponse {
  readonly status: number;
  readonly ok: boolean;
  readonly headers: Record<string, string>;
  text(): string;
  json<T = unknown>(): T;
  /** Raw bytes (text bodies are UTF-8 encoded). */
  bytes(): Uint8Array;
}

/** The in-world handle for an `http.client` resource. */
export interface HttpClientHandle {
  fetch(url: string, init?: FetchInit): Promise<FetchResponse>;
}
