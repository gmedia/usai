// Resource declarations (`GOAL.md` §23–§25, contract C18). Each is a distinct
// persistence class with a stated contract; there is no generic
// `persistent(() => …)`.

import type { ResourceDeclaration } from "./declarations.ts";

/** Options for `cache.local`.
 *
 * @category Resources
 */
export interface CacheLocalOptions {
  /** Bound on entries; the least recently used go first. */
  maxEntries?: number;
}

/** Shared across worlds, local to the runtime, not durable, may disappear on
 * restart.
 *
 * @category Resources
 */
export interface CacheLocalDeclaration<Name extends string = string>
  extends ResourceDeclaration<Name, CacheLocalHandle> {
  readonly kind: "cache.local";
}

/**
 * Cache resources.
 *
 * @example
 * ```ts
 * const hits = cache.local("hits", { maxEntries: 10_000 });
 * export const count = http.post("/hits", { resources: [hits] }, async (ctx) => ({
 *   total: await ctx.resources.hits.increment("total"),
 * }));
 * ```
 *
 * @category Resources
 */
export const cache = {
  /** A runtime-local cache: shared by every world in this process (across
   * revisions too), never persisted, gone on restart, not shared between
   * replicas. Each call is one leased operation. The in-world handle is
   * {@link CacheLocalHandle}. */
  local<const N extends string>(
    name: N,
    options: CacheLocalOptions = {},
  ): CacheLocalDeclaration<N> {
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

/** The in-world handle for a `cache.local` resource (`ctx.resources.<name>`).
 *
 * @category Resources
 */
export interface CacheLocalHandle {
  /** The value, or `null` when absent or expired. */
  get<T = unknown>(key: string): Promise<T | null>;
  /** Store a JSON value, optionally for `ttlMs`. */
  set(key: string, value: unknown, options?: { ttlMs?: number }): Promise<boolean>;
  /** Remove a key; `true` when something was removed. */
  delete(key: string): Promise<boolean>;
  /** Atomically add `by` (default 1) and return the new value; a missing key starts at 0. */
  increment(key: string, by?: number): Promise<number>;
  /** Remove every entry. */
  clear(): Promise<boolean>;
}

/** Options for {@link postgres}. The URL itself is never in the
 * declaration: it comes from the environment at activation.
 *
 * @category Resources
 */
export interface PostgresOptions {
  /** Environment variable holding the connection URL. Default `DATABASE_URL`. */
  urlEnv?: string;
  pool?: {
    /** Connections in the pool (default 16). A world that needs one while
     * all are leased waits, bounded by its deadline. */
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
 * connection; reuse follows terminal proof (contract C5).
 *
 * @category Resources
 */
export interface PostgresDeclaration<Name extends string = string>
  extends ResourceDeclaration<Name, PostgresHandle> {
  readonly kind: "postgres";
}

/**
 * Declare a PostgreSQL resource. The runtime owns the pool for its whole
 * lifetime; a workload that lists the resource gets a {@link PostgresHandle}
 * at `ctx.resources.<name>`, and every statement leases one connection for
 * exactly that operation. The connection returns to the pool only after a
 * **terminal outcome** (the result or the error arrived); a world that
 * dies mid-statement proves nothing about the connection, so it is
 * quarantined, then removed and replaced, never reused. Connection loss and
 * pool exhaustion surface to HTTP callers as 503, not 500.
 *
 * Migrations are SQL files matched by the module's `migrations` globs,
 * applied by `usai db migrate` (never at startup). The same resource can
 * be declared by several modules with the same configuration.
 *
 * @param name Unique within the application; the handle is
 * `ctx.resources[name]`, typed when the workload lists this declaration
 * under `resources`.
 *
 * @example
 * ```ts
 * export const db = postgres("db");                          // reads DATABASE_URL; `ctx.resources.db`
 * export const getUser = http.get("/users/:id", { params: Id, resources: [db] }, async (ctx) => {
 *   const user = await ctx.resources.db.one<User>("select * from users where id = $1", [ctx.params.id]);
 *   if (!user) throw errors.notFound();
 *   return user;
 * });
 * ```
 *
 * @category Resources
 */
export function postgres<const N extends string>(
  name: N,
  options: PostgresOptions = {},
): PostgresDeclaration<N> {
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

/** A statement parameter (`$1`, `$2`, …): scalars bind to their SQL type,
 * objects and arrays bind as JSON (`jsonb`); cast in SQL when a column
 * needs something else (`$1::uuid[]`).
 *
 * @category Resources
 */
export type SqlParam =
  | string
  | number
  | boolean
  | null
  | Uint8Array
  | Record<string, unknown>
  | unknown[];

/** The statements available on a connection. Rows are plain objects keyed
 * by column name; values arrive as JSON (uuid, timestamptz and numeric
 * as strings, integers and floats as numbers, json/jsonb as values, `bytea`
 * as a base64 string — `bytes.fromBase64` turns it back into a
 * `Uint8Array`). A `Uint8Array` parameter binds to a `bytea` column.
 *
 * @category Resources
 */
export interface SqlExecutor {
  // The parameter list is `readonly`: nothing here mutates it, and every
  // query builder used as a compiler (kysely, drizzle) hands back a
  // `readonly unknown[]`, which would otherwise need a cast at each call.
  /** Run a statement and return every row. */
  query<T = Record<string, unknown>>(sql: string, params?: readonly SqlParam[]): Promise<T[]>;
  /** Run a statement and return the first row, or `null`. */
  one<T = Record<string, unknown>>(sql: string, params?: readonly SqlParam[]): Promise<T | null>;
  /** Run a statement and return the number of rows affected. */
  execute(sql: string, params?: readonly SqlParam[]): Promise<number>;
}

/** The in-world handle for a `postgres` resource. Rows are plain objects
 * keyed by column name; values are JSON (uuid/timestamps as strings). Each
 * statement leases its own connection; `transaction` pins one for the
 * callback and commits when it returns, rolls back when it throws. A world
 * that ends with the transaction still open is a lifecycle error, and the
 * runtime rolls back on its behalf.
 *
 * @category Resources
 */
export interface PostgresHandle extends SqlExecutor {
  /** One transaction on one pinned connection: `BEGIN`, the callback's
   * statements on `tx`, then `COMMIT` when it returns or `ROLLBACK` when it
   * throws (the error is rethrown). The transaction is live work owned by
   * this world: cancellation rolls it back, and a finite world that ends
   * with it still open is a lifecycle error, rolled back by the runtime. */
  transaction<T>(fn: (tx: SqlExecutor) => Promise<T>): Promise<T>;
}

/** Options for {@link httpClient}. Secrets never go in the declaration:
 * name the environment variables that hold them.
 *
 * @category Resources */
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
  /** Let a client **without** a `baseUrl` reach loopback, private,
   * link-local and unique-local addresses. Off by default.
   *
   * The only reason a client without a `baseUrl` exists is that the
   * destination comes from the application's own data — a
   * tenant-configured webhook — which makes it attacker-influenced by
   * construction, and `http://169.254.169.254/…`, `http://127.0.0.1:3900`
   * and an internal service's name are all requests your application would
   * make on the caller's behalf. Refused with `destination_refused`.
   *
   * Set it only when the client really does call internal addresses chosen
   * at runtime. A client that names its destination (`baseUrl` /
   * `baseUrlEnv`) is pinned to one origin already and is never checked. */
  allowPrivateNetwork?: boolean;
}

/** Outbound HTTP, declared: the runtime owns the client (pool, TLS roots,
 * timeouts), every request is an operation owned by the world, and the
 * destination is visible in `usai graph` and the API docs. There is no
 * global `fetch` inside a world.
 *
 * @category Resources
 */
export interface HttpClientDeclaration<Name extends string = string>
  extends ResourceDeclaration<Name, HttpClientHandle> {
  readonly kind: "http.client";
}

/**
 * Declare an outbound HTTP client. There is no global `fetch` in a world;
 * this is how an application calls another service, and the destination
 * is part of the application's declared shape. Each `fetch` is one leased
 * operation: cancelled with the world, bounded by the smaller of the
 * world's deadline and `timeoutMs`, counted against `maxConcurrent` (the
 * next request is refused, not queued). With `baseUrl`/`baseUrlEnv` the
 * origin is pinned and any other origin is `origin_refused` before the
 * request leaves. A non-2xx status is data (`ok: false`), not an exception;
 * connection failures and timeouts throw and map to 503 for HTTP callers.
 *
 * @example
 * ```ts
 * export const mailer = httpClient("mailer", { baseUrlEnv: "MAILER_URL", bearerTokenEnv: "MAILER_TOKEN", timeoutMs: 5_000, maxConcurrent: 8 });
 * export const send = task("send-mail", { input: Mail, resources: [mailer] }, async (ctx) => {
 *   const res = await ctx.resources.mailer.fetch("/v1/send", { method: "POST", json: ctx.input });
 *   if (!res.ok) throw errors.unavailable(`mailer answered ${res.status}`);
 * });
 * ```
 *
 * @category Resources
 */
export function httpClient<const N extends string>(
  name: N,
  options: HttpClientOptions = {},
): HttpClientDeclaration<N> {
  const config: Record<string, unknown> = {};
  if (options.baseUrl !== undefined) config["baseUrl"] = options.baseUrl;
  if (options.baseUrlEnv !== undefined) config["baseUrlEnv"] = options.baseUrlEnv;
  if (options.timeoutMs !== undefined) config["timeoutMs"] = options.timeoutMs;
  if (options.maxConcurrent !== undefined) config["maxConcurrent"] = options.maxConcurrent;
  if (options.headers !== undefined) config["headers"] = options.headers;
  if (options.bearerTokenEnv !== undefined) config["bearerTokenEnv"] = options.bearerTokenEnv;
  if (options.allowPrivateNetwork !== undefined)
    config["allowPrivateNetwork"] = options.allowPrivateNetwork;
  return {
    __usai: "resource",
    name,
    kind: "http.client",
    config,
    env: [options.baseUrlEnv, options.bearerTokenEnv].filter((v): v is string => v !== undefined),
    methods: ["fetch"],
  };
}

/** Options for {@link HttpClientHandle.fetch}.
 *
 * @category Resources
 */
export interface FetchInit {
  /** Default `GET`. */
  method?: string;
  headers?: Record<string, string>;
  /** A string body, or a JSON value (serialized, `content-type: application/json`). */
  body?: string;
  json?: unknown;
  /** Lower than the resource's timeout only. */
  timeoutMs?: number;
}

/** A completed response: the body has already arrived, so the accessors
 * are synchronous.
 *
 * @category Resources
 */
export interface FetchResponse {
  readonly status: number;
  /** `status` is 2xx. */
  readonly ok: boolean;
  /** Lower-cased header names. */
  readonly headers: Record<string, string>;
  text(): string;
  /** `JSON.parse` of the body. */
  json<T = unknown>(): T;
  /** Raw bytes (text bodies are UTF-8 encoded). */
  bytes(): Uint8Array;
}

/** The in-world handle for an `http.client` resource (`ctx.resources.<name>`).
 *
 * @category Resources
 */
export interface HttpClientHandle {
  /** One request. `url` is a path (relative to `baseUrl`) or an absolute
   * URL on the pinned origin. */
  fetch(url: string, init?: FetchInit): Promise<FetchResponse>;
}
