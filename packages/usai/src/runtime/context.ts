// The context every handler receives, built on top of the guest bridge
// (`docs/GUEST-ABI.md`). Everything asynchronous here is a host-owned
// operation; nothing escapes the world's ownership.

import type { ResourceDeclaration, Workload } from "../declarations.ts";
import type { CacheLocalHandle, PostgresHandle } from "../resources.ts";
import { UsaiError } from "../errors.ts";

declare global {
  // Provided by the guest bridge (`docs/GUEST-ABI.md`); declared here so the
  // SDK does not depend on DOM or Node typings for the guest surface.
  function setTimeout(fn: (...args: unknown[]) => void, ms?: number, ...args: unknown[]): number;
  function clearTimeout(id: number | undefined): void;
  function setInterval(fn: (...args: unknown[]) => void, ms?: number, ...args: unknown[]): unknown;
  function clearInterval(handle: unknown): void;
  function queueMicrotask(fn: () => void): void;
  function atob(data: string): string;
  function btoa(data: string): string;
  interface ConsoleLike {
    debug(...args: unknown[]): void;
    info(...args: unknown[]): void;
    log(...args: unknown[]): void;
    warn(...args: unknown[]): void;
    error(...args: unknown[]): void;
  }
  var console: ConsoleLike;
  class TextEncoder {
    encode(input?: string): Uint8Array;
  }
  class TextDecoder {
    constructor(label?: string);
    decode(input?: Uint8Array): string;
  }
  // Installed by the host before the application module evaluates.
  var __usai: {
    op(kind: string, payload: string): Promise<string>;
    onCancel(fn: (reason: string) => void): void;
    isCancelled(): boolean;
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
  readonly env: Record<string, string>;
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

function postgresHandle(name: string): PostgresHandle {
  return {
    query: (sql, params = []) => resourceCall(name, "query", { sql, params }) as Promise<never[]>,
    one: (sql, params = []) => resourceCall(name, "one", { sql, params }) as Promise<never>,
    execute: (sql, params = []) => resourceCall(name, "execute", { sql, params }) as Promise<number>,
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
      : genericHandle(declaration);
  }
  return out;
}

export function parseDurationMs(value: string | number): number {
  if (typeof value === "number") return value;
  const match = /^(\d+(?:\.\d+)?)\s*(ms|s|m|h)?$/.exec(value.trim());
  if (!match) throw new UsaiError("invalid_duration", 500, `invalid duration ${JSON.stringify(value)}`);
  const n = Number(match[1]);
  const unit = match[2] ?? "ms";
  return unit === "s" ? n * 1000 : unit === "m" ? n * 60_000 : unit === "h" ? n * 3_600_000 : n;
}

export function makeBase(resources: readonly ResourceDeclaration[], env: Record<string, string>): BaseContext {
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
