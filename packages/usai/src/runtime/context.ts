// The context every handler receives, built on top of the guest bridge
// (`docs/GUEST-ABI.md`). Everything asynchronous here is a host-owned
// operation; nothing escapes the world's ownership.

import type { ResourceDeclaration, Workload } from "../declarations.ts";
import type { CacheLocalHandle, FetchInit, FetchResponse, HttpClientHandle, PostgresHandle, SqlExecutor } from "../resources.ts";
import { UsaiError } from "../errors.ts";

/** What `ctx.log` and `console` offer inside a world.
 *
 * @category Context
 */
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

/** `ctx.signal`: aborts when this world is cancelled — the client went
 * away, the deadline passed, the revision drained, or the owner of an
 * `invoke` was cancelled. Pending host operations reject with
 * `cancelled` at the same moment; the signal is for the handler's own
 * loops and cleanup.
 *
 * @category Context
 */
export interface UsaiAbortSignal {
  readonly aborted: boolean;
  /** Why, once aborted (`deadline exceeded`, `cancelled by owner`, …). */
  readonly reason: string | undefined;
  /** Runs at abort (immediately when already aborted). */
  addEventListener(type: "abort", listener: (reason: string) => void): void;
  /** Throws a `cancelled` (499) {@link UsaiError} once aborted. */
  throwIfAborted(): void;
}

/** `ctx.tasks`: the two ways to start a task, and the whole difference
 * between them is who owns the child world.
 *
 * @category Context
 */
export interface TaskHandle {
  /** An **owned** invocation: the task runs in a fresh world, this world waits
   * for its result, and cancelling this world cancels the child. The
   * child's thrown {@link UsaiError} is rethrown here. Use it when the
   * response depends on the task. */
  invoke<T = unknown>(task: Workload, input?: unknown): Promise<T>;
  /** An **ownership transfer**: the task runtime owns the child, which
   * starts once this world commits (its handler returned; for HTTP, the
   * response is committed) — a world that throws hands nothing off. This
   * world may end. Resolves with the child's id as soon as the hand-off is
   * accepted; rejects with `capacity_exhausted` when the task's
   * `concurrency` is full and `unknown_task` for a name that does not
   * exist. Nobody receives the child's return value. Not durable across a
   * runtime restart (publish to a queue for that). Declare the edge with
   * `dispatches(from, task)`. */
  dispatch(task: Workload, input?: unknown): Promise<{ id: string }>;
}

/**
 * What every handler receives, whatever the workload kind. Everything
 * asynchronous here is a host operation **owned by this world**: it is
 * cancelled when the world is, and a finite world may not end while one is
 * still pending (that is a lifecycle error with a diagnostic, not a leak).
 * Nothing on the context survives the world.
 *
 * @category Context
 */
export interface BaseContext {
  /** The declared resources by name, as their in-world handles
   * ({@link PostgresHandle}, {@link CacheLocalHandle}, {@link HttpClientHandle}).
   * Reading an undeclared name throws `resource_not_declared` with the fix. */
  readonly resources: Record<string, unknown>;
  /** Start tasks: owned (`invoke`) or transferred (`dispatch`). */
  readonly tasks: TaskHandle;
  /** Publish to a queue topic; durable once the insert commits. */
  readonly queue: import("../queue.ts").QueueHandle;
  /** Aborts when this world is cancelled. */
  readonly signal: UsaiAbortSignal;
  /** The declared environment, parsed: `env.int()` gives a number,
   * `env.bool()` a boolean, `env.optional(...)` may be undefined. The
   * static type is the union of those; narrow per key, or type it once
   * with `const e = ctx.env as EnvValues<typeof spec>` (the context does
   * not carry the declaration's type). */
  readonly env: Record<string, string | number | boolean | undefined>;
  /** Structured logging; lines carry the workload and world ids and reach
   * the runtime's log (`target: "app"`). `console.*` is the same. */
  readonly log: Pick<ConsoleLike, "debug" | "info" | "warn" | "error">;
  /** A timer owned by this world (`"500ms"`, `"2s"`, or milliseconds). It
   * resolves early when the world is asked to stop, so a service loop can
   * `await ctx.sleep("1s")` and then check `ctx.signal.aborted`. */
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
