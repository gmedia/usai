// The context every handler receives, built on top of the guest bridge
// (`docs/GUEST-ABI.md`). Everything asynchronous here is a host-owned
// operation; nothing escapes the world's ownership.

import type { ResourceDeclaration, Workload } from "../declarations.ts";
import type { CacheLocalHandle, FetchInit, FetchResponse, HttpClientHandle, PostgresHandle, SqlExecutor } from "../resources.ts";
import { UsaiError } from "../errors.ts";

/** What `ctx.log` and `console` offer inside a world. */
export interface ConsoleLike {
  debug(...args: unknown[]): void;
  info(...args: unknown[]): void;
  log(...args: unknown[]): void;
  warn(...args: unknown[]): void;
  error(...args: unknown[]): void;
}

declare global {
  // console / TextEncoder / TextDecoder / URL / structuredClone / timers are
  // declared once, for the SDK and for applications, in `globals.d.ts`
  // (`@sakaladev/usai/globals`).
  // Installed by the host before the application module evaluates.
  var __usai: {
    op(kind: string, payload: string): Promise<string>;
    onCancel(fn: (reason: string) => void): void;
    isCancelled(): boolean;
    hold(kind: string): { release(): void };
  };
  var __usai_sdk: unknown;
  var __usai_app: unknown;
}

export interface UsaiAbortSignal {
  readonly aborted: boolean;
  readonly reason: string | undefined;
  addEventListener(type: "abort", listener: (reason: string) => void): void;
  throwIfAborted(): void;
}

export interface TaskHandle {
  /** Owned invocation: the current world waits for the child. */
  invoke<T = unknown>(task: Workload, input?: unknown): Promise<T>;
  /** Ownership transfer: the task runtime owns the child; the parent may finish. */
  dispatch(task: Workload, input?: unknown): Promise<{ id: string }>;
}

export interface BaseContext {
  readonly resources: Record<string, unknown>;
  readonly tasks: TaskHandle;
  readonly queue: import("../queue.ts").QueueHandle;
  readonly signal: UsaiAbortSignal;
  /** The declared environment, typed: `env.int()` gives a number,
   * `env.bool()` a boolean, `env.optional(...)` may be undefined. Narrow
   * per key, or type it once: `const e = ctx.env as EnvValues<typeof spec>`. */
  readonly env: Record<string, string | number | boolean | undefined>;
  readonly log: Pick<ConsoleLike, "debug" | "info" | "warn" | "error">;
  sleep(duration: string | number): Promise<void>;
}

export async function op<T = unknown>(kind: string, payload: unknown): Promise<T> {
  const raw = await globalThis.__usai.op(kind, typeof payload === "string" ? payload : JSON.stringify(payload));
  return raw === "" ? (undefined as T) : (JSON.parse(raw) as T);
}

export function makeSignal(): UsaiAbortSignal {
  const listeners: Array<(reason: string) => void> = [];
  let reason: string | undefined;
  globalThis.__usai.onCancel((r) => {
    reason = r;
    for (const fn of listeners) {
      try { fn(r); } catch { /* listener errors never break cancellation */ }
    }
  });
  return {
    get aborted() { return reason !== undefined; },
    get reason() { return reason; },
    addEventListener(_type, listener) {
      if (reason !== undefined) listener(reason);
      else listeners.push(listener);
    },
    throwIfAborted() {
      if (reason !== undefined) throw new UsaiError("cancelled", 499, `work was cancelled: ${reason}`);
    },
  };
}

function resourceCall(name: string, method: string, args: Record<string, unknown>): Promise<unknown> {
  return op("resource", { name, method, args });
}

function cacheLocalHandle(name: string): CacheLocalHandle {
  return {
    get: (key) => resourceCall(name, "get", { key }) as Promise<never>,
    set: (key, value, options) => resourceCall(name, "set", { key, value, ...(options?.ttlMs !== undefined ? { ttlMs: options.ttlMs } : {}) }) as Promise<boolean>,
    delete: (key) => resourceCall(name, "delete", { key }) as Promise<boolean>,
    increment: (key, by) => resourceCall(name, "increment", { key, ...(by !== undefined ? { by } : {}) }) as Promise<number>,
    clear: () => resourceCall(name, "clear", {}) as Promise<boolean>,
  };
}

function sqlExecutor(name: string, lease?: number): SqlExecutor {
  const extra = lease === undefined ? {} : { lease };
  return {
    query: (sql, params = []) => resourceCall(name, "query", { sql, params, ...extra }) as Promise<never[]>,
    one: (sql, params = []) => resourceCall(name, "one", { sql, params, ...extra }) as Promise<never>,
    execute: (sql, params = []) => resourceCall(name, "execute", { sql, params, ...extra }) as Promise<number>,
  };
}

function postgresHandle(name: string): PostgresHandle {
  return {
    ...sqlExecutor(name),
    async transaction(fn) {
      const { lease } = (await resourceCall(name, "begin", {})) as { lease: number };
      // The open transaction is live asynchronous work: a finite world that
      // ends before commit/rollback is diagnosed, and the runtime rolls back.
      const held = globalThis.__usai.hold("postgres.transaction");
      try {
        const result = await fn(sqlExecutor(name, lease));
        await resourceCall(name, "commit", { lease });
        return result;
      } catch (error) {
        await resourceCall(name, "rollback", { lease }).catch(() => undefined);
        throw error;
      } finally {
        held.release();
      }
    },
  };
}

interface RawFetchResponse {
  status: number;
  ok: boolean;
  headers: Record<string, string>;
  body: { text?: string; base64?: string };
}

function httpClientHandle(name: string): HttpClientHandle {
  return {
    async fetch(url: string, init: FetchInit = {}): Promise<FetchResponse> {
      const headers = { ...(init.headers ?? {}) };
      let body = init.body;
      if (init.json !== undefined) {
        body = JSON.stringify(init.json);
        if (!Object.keys(headers).some((h) => h.toLowerCase() === "content-type")) headers["content-type"] = "application/json";
      }
      const args: Record<string, unknown> = { url, headers };
      if (init.method !== undefined) args["method"] = init.method;
      if (body !== undefined) args["body"] = body;
      if (init.timeoutMs !== undefined) args["timeoutMs"] = init.timeoutMs;
      const raw = (await resourceCall(name, "fetch", args)) as RawFetchResponse;
      const text = () => raw.body.text ?? new TextDecoder().decode(bytes());
      const bytes = () => {
        if (raw.body.base64 !== undefined) {
          const bin = atob(raw.body.base64);
          const out = new Uint8Array(bin.length);
          for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
          return out;
        }
        return new TextEncoder().encode(raw.body.text ?? "");
      };
      return {
        status: raw.status,
        ok: raw.ok,
        headers: raw.headers,
        text,
        json: <T,>() => JSON.parse(text()) as T,
        bytes,
      };
    },
  };
}

function genericHandle(declaration: ResourceDeclaration): Record<string, (args?: Record<string, unknown>) => Promise<unknown>> {
  const handle: Record<string, (args?: Record<string, unknown>) => Promise<unknown>> = {};
  for (const method of declaration.methods) {
    handle[method] = (args = {}) => resourceCall(declaration.name, method, args);
  }
  return handle;
}

export function makeResources(declarations: readonly ResourceDeclaration[]): Record<string, unknown> {
  const out: Record<string, unknown> = {};
  for (const declaration of declarations) {
    out[declaration.name] =
      declaration.kind === "cache.local" ? cacheLocalHandle(declaration.name)
      : declaration.kind === "postgres" ? postgresHandle(declaration.name)
      : declaration.kind === "http.client" ? httpClientHandle(declaration.name)
      : genericHandle(declaration);
  }
  // A resource the workload did not declare is a lifecycle mistake, not
  // `undefined`: say so, with the fix, instead of failing later on
  // "cannot read property 'query' of undefined".
  const declared = Object.keys(out);
  return new Proxy(out, {
    get(target, key, receiver) {
      if (typeof key === "string" && !(key in target) && key !== "then" && key !== "toJSON") {
        const hint = declared.length ? `declared here: ${declared.join(", ")}` : "this workload declares no resources";
        throw new UsaiError("resource_not_declared", 500, `resource "${key}" is not declared on this workload (${hint}); add it to the workload's \`resources: [...]\``);
      }
      return Reflect.get(target, key, receiver);
    },
  });
}

export function parseDurationMs(value: string | number): number {
  if (typeof value === "number") return value;
  const match = /^(\d+(?:\.\d+)?)\s*(ms|s|m|h)?$/.exec(value.trim());
  if (!match) throw new UsaiError("invalid_duration", 500, `invalid duration ${JSON.stringify(value)}`);
  const n = Number(match[1]);
  const unit = match[2] ?? "ms";
  return unit === "s" ? n * 1000 : unit === "m" ? n * 60_000 : unit === "h" ? n * 3_600_000 : n;
}

export function makeBase(resources: readonly ResourceDeclaration[], env: Record<string, string | number | boolean | undefined>): BaseContext {
  return {
    resources: makeResources(resources),
    tasks: {
      invoke: (task, input) => op("task.invoke", { name: task.name, input: input ?? null }),
      dispatch: (task, input) => op("task.dispatch", { name: task.name, input: input ?? null }),
    },
    queue: {
      publish: (topic, message, options) =>
        op("queue.publish", { topic, message: message ?? null, ...(options?.delayMs !== undefined ? { delayMs: options.delayMs } : {}), ...(options?.database ? { database: options.database.name } : {}) }),
    },
    signal: makeSignal(),
    env,
    log: console,
    sleep: (duration) => new Promise<void>((resolve) => setTimeout(() => resolve(), parseDurationMs(duration))),
  };
}
