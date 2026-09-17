// Connection-bound workloads (`GOAL.md` §20–§21): streams and sockets. The
// world lives as long as the connection; `ctx.state` is connection-local
// and ends with it.

import type { AnySchema, Output } from "./schema.ts";
import type { AuthDeclaration, Method, ResourceDeclaration, Workload, WorkloadPolicies } from "./declarations.ts";
import type { BaseContext } from "./runtime/context.ts";
import type { HttpContracts } from "./declarations.ts";

export interface StreamHandle {
  /** Commit status/headers before the first chunk (optional). */
  start(options?: { status?: number; headers?: Record<string, string> }): Promise<void>;
  /** One chunk. Commits a 200 head on first use. */
  send(chunk: string | Uint8Array): Promise<void>;
  /** Server-sent event: `event:` + `data:` lines. */
  event(name: string, data: unknown): Promise<void>;
}

export interface StreamContext extends BaseContext {
  readonly method: Method;
  readonly path: string;
  readonly url: string;
  readonly params: Record<string, string>;
  readonly query: Record<string, string | string[]>;
  readonly headers: Record<string, string>;
}

export interface StreamOptions extends WorkloadPolicies {
  method?: Method;
  auth?: AuthDeclaration;
  resources?: ResourceDeclaration[];
  params?: HttpContracts["params"];
  query?: HttpContracts["query"];
}

function stream(path: string, options: StreamOptions, handler: (ctx: StreamContext, stream: StreamHandle) => unknown): Workload {
  const method = options.method ?? "GET";
  const contracts: Workload["contracts"] = {};
  if (options.params) contracts.params = options.params;
  if (options.query) contracts.query = options.query;
  const policies: WorkloadPolicies = {};
  if (options.timeout !== undefined) policies.timeout = options.timeout;
  if (options.concurrency !== undefined) policies.concurrency = options.concurrency;
  return {
    __usai: "workload",
    kind: "stream",
    name: `${method} ${path}`,
    trigger: { method, path },
    contracts,
    errors: [],
    ...(options.auth ? { auth: options.auth } : {}),
    resources: options.resources ?? [],
    dispatches: [],
    policies,
    handler: handler as Workload["handler"],
  };
}

export const streams = { stream };

export interface SocketContext<Incoming, Outgoing> extends BaseContext {
  readonly path: string;
  readonly url: string;
  readonly params: Record<string, string>;
  readonly query: Record<string, string | string[]>;
  readonly headers: Record<string, string>;
  /** Connection-local mutable state: survives messages, ends with the connection. */
  readonly state: Record<string, unknown>;
  send(message: Outgoing): Promise<void>;
  close(reason?: string): Promise<void>;
  readonly message: Incoming;
  readonly closeInfo: { code: number | null; reason: string } | null;
}

export interface SocketOptions<I extends AnySchema | undefined, O extends AnySchema | undefined> extends WorkloadPolicies {
  incoming?: I;
  outgoing?: O;
  auth?: AuthDeclaration;
  resources?: ResourceDeclaration[];
}

type Out<S> = S extends AnySchema ? Output<S> : unknown;

export interface SocketHandlers<I, O> {
  open?(ctx: SocketContext<I, O>): unknown;
  message?(ctx: SocketContext<I, O>): unknown;
  close?(ctx: SocketContext<I, O>): unknown;
}

export function socket<I extends AnySchema | undefined = undefined, O extends AnySchema | undefined = undefined>(
  path: string,
  options: SocketOptions<I, O>,
  handlers: SocketHandlers<Out<I>, Out<O>>,
): Workload {
  const contracts: Workload["contracts"] = {};
  if (options.incoming) contracts.message = options.incoming;
  if (options.outgoing) contracts.response = { 200: options.outgoing };
  const policies: WorkloadPolicies = {};
  if (options.concurrency !== undefined) policies.concurrency = options.concurrency;
  return {
    __usai: "workload",
    kind: "socket",
    name: path,
    trigger: { path },
    contracts,
    errors: [],
    ...(options.auth ? { auth: options.auth } : {}),
    resources: options.resources ?? [],
    dispatches: [],
    policies,
    handler: handlers as unknown as Workload["handler"],
  };
}
