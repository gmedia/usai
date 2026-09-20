// Connection-bound workloads (`GOAL.md` §20–§21): streams and sockets. The
// world lives as long as the connection; `ctx.state` is connection-local
// and ends with it.

import type { AnySchema, Output } from "./schema.ts";
import type {
  AuthDeclaration,
  Method,
  ResourceDeclaration,
  ResourcesOf,
  Workload,
  WorkloadPolicies,
} from "./declarations.ts";
import type { BaseContext } from "./runtime/context.ts";
import type { HttpContracts } from "./declarations.ts";

/** The second argument of an `http.stream` handler: the response, chunk
 * by chunk. The first `send`/`event`/`start` **commits** the status and
 * headers; after that the world is bound to the connection and ends when
 * the handler returns, the client disconnects (the world is cancelled), or
 * the revision drains.
 *
 * @category Streams and WebSockets
 */
export interface StreamHandle {
  /** Commit status/headers before the first chunk (optional). */
  start(options?: { status?: number; headers?: Record<string, string> }): Promise<void>;
  /** One chunk. Commits a 200 head on first use. */
  send(chunk: string | Uint8Array): Promise<void>;
  /** Server-sent event: `event:` + `data:` lines. */
  event(name: string, data: unknown): Promise<void>;
}

/** The context of a streaming request: request facts plus {@link BaseContext}.
 *
 * @category Streams and WebSockets
 */
export interface StreamContext<O extends StreamOptions = StreamOptions> extends BaseContext {
  /** The request's id (`x-request-id`, the client's or minted). */
  readonly requestId: string;
  /** The declared resources, typed by name from `resources: [...]`. */
  readonly resources: ResourcesOf<O["resources"]>;
  readonly method: Method;
  readonly path: string;
  readonly url: string;
  /** Path parameters, validated against `params` (strings when undeclared). */
  readonly params: OutputOf<O["params"], Record<string, string>>;
  /** Query, validated against `query`. */
  readonly query: OutputOf<O["query"], Record<string, string | string[]>>;
  readonly headers: Record<string, string>;
  /** The principal the `auth` declaration resolved; `undefined` without one. */
  readonly auth: O["auth"] extends AuthDeclaration<infer P> ? P : undefined;
}

type OutputOf<S, Fallback> = S extends AnySchema ? Output<S> : Fallback;

/** Options for `http.stream`.
 *
 * @category Streams and WebSockets
 */
export interface StreamOptions<R extends ResourceDeclaration[] = ResourceDeclaration[]>
  extends WorkloadPolicies {
  /** Default `GET`. */
  method?: Method;
  summary?: string;
  description?: string;
  auth?: AuthDeclaration;
  resources?: R;
  params?: HttpContracts["params"];
  query?: HttpContracts["query"];
}

/**
 * Declare a streaming endpoint (`http.stream`): a **connection-bound**
 * world that lives until the handler returns. `params` and `query` are
 * validated before the world exists; `timeout` bounds the whole stream.
 * Chunks are `text/event-stream` by default (`stream.event(name, data)`
 * writes one server-sent event); set `content-type` in `start` for
 * anything else.
 *
 * @example
 * ```ts
 * export const events = http.stream("/events", { resources: [cache] }, async (ctx, stream) => {
 *   while (!ctx.signal.aborted) {
 *     await stream.event("tick", { total: await ctx.resources.cache.get("total") });
 *     await ctx.sleep("1s");
 *   }
 * });
 * ```
 */
function stream<O extends StreamOptions>(
  path: string,
  options: O,
  handler: (ctx: StreamContext<O>, stream: StreamHandle) => unknown,
): Workload {
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
    ...(options.summary ? { summary: options.summary } : {}),
    ...(options.description ? { description: options.description } : {}),
    trigger: { method, path },
    contracts,
    errors: [],
    ...(options.auth ? { auth: options.auth } : {}),
    resources: options.resources ?? [],
    dispatches: [],
    publishes: [],
    policies,
    handler: handler as Workload["handler"],
  };
}

/** @internal Reached as `http.stream`. */
export const streams = { stream };

/** The context of a WebSocket connection, shared by `open`, `message` and
 * `close`: request facts, `send`/`close`, connection-local `state`, and
 * in `message` the validated incoming `message`.
 *
 * @category Streams and WebSockets
 */
export interface SocketContext<Incoming, Outgoing, R = ResourceDeclaration[], A = unknown>
  extends BaseContext {
  /** The upgrade request's id (`x-request-id`, the client's or minted). */
  readonly requestId: string;
  readonly resources: ResourcesOf<R>;
  readonly path: string;
  readonly url: string;
  readonly params: Record<string, string>;
  readonly query: Record<string, string | string[]>;
  readonly headers: Record<string, string>;
  /** The principal the `auth` declaration resolved; `undefined` without one. */
  readonly auth: A;
  /** Connection-local mutable state: survives messages, ends with the connection. */
  readonly state: Record<string, unknown>;
  /** Send one message (validated against `outgoing` when declared). */
  send(message: Outgoing): Promise<void>;
  /** Close the connection; the world ends after `close` ran. */
  close(reason?: string): Promise<void>;
  /** The current message (in the `message` handler). */
  readonly message: Incoming;
  /** Why the connection closed (in the `close` handler). */
  readonly closeInfo: { code: number | null; reason: string } | null;
}

/** Options for {@link socket}.
 *
 * @category Streams and WebSockets
 */
export interface SocketOptions<
  I extends AnySchema | undefined,
  O extends AnySchema | undefined,
  R extends ResourceDeclaration[] = ResourceDeclaration[],
  A extends AuthDeclaration | undefined = AuthDeclaration | undefined,
> extends WorkloadPolicies {
  summary?: string;
  description?: string;
  /** Schema for messages from the client. An invalid message is answered
   * with a `validation_failed` error envelope and dropped; the connection
   * stays open. */
  incoming?: I;
  /** Schema for messages to the client. */
  outgoing?: O;
  auth?: A;
  resources?: R;
}

type Out<S> = S extends AnySchema ? Output<S> : unknown;

/** The three moments of a connection.
 *
 * @category Streams and WebSockets
 */
export interface SocketHandlers<I, O, R = ResourceDeclaration[], A = unknown> {
  /** After the upgrade. */
  open?(ctx: SocketContext<I, O, R, A>): unknown;
  /** Once per incoming message, in order. */
  message?(ctx: SocketContext<I, O, R, A>): unknown;
  /** After the connection closed, whoever closed it. */
  close?(ctx: SocketContext<I, O, R, A>): unknown;
}

/**
 * Declare a WebSocket endpoint: one **connection-bound** world per
 * connection, from the upgrade to the close. `ctx.state` is the
 * connection's mutable memory and ends with it; nothing is shared between
 * connections except through resources. A client disconnect cancels the
 * world; a draining revision closes the socket with 1012 (service
 * restart) and `close` runs. `concurrency` bounds open connections.
 *
 * @example
 * ```ts
 * export const chat = socket("/chat", { incoming: ChatMessage, outgoing: ChatMessage, resources: [cache] }, {
 *   open: async (ctx) => { ctx.state.joined = Date.now(); },
 *   message: async (ctx) => { await ctx.send({ ...ctx.message, echoed: true }); },
 *   close: async (ctx) => { await ctx.resources.cache.increment("closed"); },
 * });
 * ```
 *
 * @category Streams and WebSockets
 */
export function socket<
  I extends AnySchema | undefined = undefined,
  O extends AnySchema | undefined = undefined,
  R extends ResourceDeclaration[] = ResourceDeclaration[],
  A extends AuthDeclaration | undefined = undefined,
>(
  path: string,
  options: SocketOptions<I, O, R, A>,
  handlers: SocketHandlers<Out<I>, Out<O>, R, A extends AuthDeclaration<infer P> ? P : undefined>,
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
    ...(options.summary ? { summary: options.summary } : {}),
    ...(options.description ? { description: options.description } : {}),
    trigger: { path },
    contracts,
    errors: [],
    ...(options.auth ? { auth: options.auth } : {}),
    resources: options.resources ?? [],
    dispatches: [],
    publishes: [],
    policies,
    handler: handlers as unknown as Workload["handler"],
  };
}
