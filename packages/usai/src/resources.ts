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
