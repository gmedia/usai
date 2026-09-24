// Connection-bound workloads (`GOAL.md` §20–§21): streams and sockets. The
// world lives as long as the connection; `ctx.state` is connection-local
// and ends with it.

import type { AnySchema, Output } from "./schema.ts";
import type {
  AuthDeclaration,
  AuthResourcesOf,
  Method,
  ResourceDeclaration,
  ResourcesOf,
  ResponseHeaderDocs,
  Workload,
  WorkloadPolicies,
} from "./declarations.ts";
import { withAuthResources } from "./declarations.ts";
import type { BaseContext } from "./runtime/context.ts";
import type { HttpContracts, NoExtraKeys } from "./declarations.ts";

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
  /** Server-sent event: `event:` + `data:` lines (`data` is JSON unless it
   * is already a string; a multi-line string becomes several `data:`
   * lines). `id` sets the event's `id:` — the browser's `EventSource` sends
   * the last one back as the `last-event-id` header when it reconnects, so
   * a handler that reads `ctx.headers["last-event-id"]` resumes where the
   * client left off; `retry` (milliseconds) tells the browser how long to
   * wait before reconnecting. */
  event(
    name: string,
    data: unknown,
    options?: { id?: string | number; retry?: number },
  ): Promise<void>;
}

/** The context of a streaming request: request facts plus {@link BaseContext}.
 *
 * @category Streams and WebSockets
 */
export interface StreamContext<O extends StreamOptions = StreamOptions> extends BaseContext {
  /** The request's id (`x-request-id`, the client's or minted). */
  readonly requestId: string;
  /** The declared resources, typed by name from `resources: [...]`. */
  readonly resources: ResourcesOf<O["resources"]> & AuthResourcesOf<O["auth"]>;
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
  /** The stream's media type: `text/event-stream` (default; an SSE
   * endpoint), `text/csv`, `application/x-ndjson`, … Sets the response
   * `content-type` unless `stream.start({ headers })` says otherwise, and is
   * what the OpenAPI document and the reference say the endpoint streams. */
  contentType?: string;
  /** Response headers the stream sets (`content-disposition` for a download), documented. */
  responseHeaders?: ResponseHeaderDocs;
  /** The OpenAPI `operationId` (a generated client's method name). */
  operationId?: string;
  /** The server-sent events this stream emits, by name, with the schema of
   * each `data:` payload: `events: { tick: z.object({ i: z.number() }) }`.
   * `stream.event("tick", data)` validates `data` against it (a mismatch is
   * a `500 event_contract_violation` — the stream ends early and the log
   * says which event), and the OpenAPI document names them as
   * `components.schemas.<OperationId>EventTick` with the event list in the
   * response description, so a client knows what to `addEventListener` for. */
  events?: Record<string, AnySchema>;
}

/**
 * Declare a streaming endpoint (`http.stream`): a **connection-bound**
 * world that lives until the handler returns. `params` and `query` are
 * validated before the world exists; `timeout` bounds the whole stream.
 * Chunks are `text/event-stream` by default (`stream.event(name, data, { id })`
 * writes one server-sent event; a reconnecting `EventSource` sends the last
 * `id` back as `ctx.headers["last-event-id"]`); set `content-type` in
 * `start` for anything else. A client that leaves cancels the world — the
 * normal end of a stream, not a failure.
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
  options: NoExtraKeys<O, StreamOptions>,
  handler: (ctx: StreamContext<O>, stream: StreamHandle) => unknown,
): Workload {
  const method = options.method ?? "GET";
  const contracts: Workload["contracts"] = {};
  if (options.params) contracts.params = options.params;
  if (options.query) contracts.query = options.query;
  if (options.events) contracts.events = options.events;
  const policies: WorkloadPolicies = {};
  if (options.timeout !== undefined) policies.timeout = options.timeout;
  if (options.concurrency !== undefined) policies.concurrency = options.concurrency;
  if (options.maxBodyBytes !== undefined) policies.maxBodyBytes = options.maxBodyBytes;
  return {
    __usai: "workload",
    kind: "stream",
    name: `${method} ${path}`,
    ...(options.summary ? { summary: options.summary } : {}),
    ...(options.description ? { description: options.description } : {}),
    trigger: { method, path, ...(options.contentType ? { contentType: options.contentType } : {}) },
    contracts,
    errors: [],
    ...(options.responseHeaders ? { responseHeaders: options.responseHeaders } : {}),
    ...(options.operationId ? { operationId: options.operationId } : {}),
    ...(options.auth ? { auth: options.auth } : {}),
    resources: withAuthResources(options.resources, options.auth),
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
export interface SocketContext<
  Incoming,
  Outgoing,
  R = ResourceDeclaration[],
  A = unknown,
  D = undefined,
  P = Record<string, string>,
  Q = Record<string, string | string[]>,
> extends BaseContext {
  /** The upgrade request's id (`x-request-id`, the client's or minted). */
  readonly requestId: string;
  /** The declared resources, plus the ones the auth scheme (`D`) leases. */
  readonly resources: ResourcesOf<R> & AuthResourcesOf<D>;
  readonly path: string;
  readonly url: string;
  /** The path parameters, typed and **validated before the world exists**
   * when `params` is declared (C6, the same as an HTTP route); a plain
   * `Record<string, string>` the handler must check itself otherwise. */
  readonly params: P;
  /** The upgrade's query string, typed and validated when `query` is
   * declared. */
  readonly query: Q;
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
  PS extends AnySchema | undefined = undefined,
  QS extends AnySchema | undefined = undefined,
> extends WorkloadPolicies {
  summary?: string;
  description?: string;
  /** Path parameters, validated **before the world exists** — a bad
   * `/live/:board` is `400` and no connection is upgraded. Without it
   * `ctx.params` is an unchecked `Record<string, string>`, which used to be
   * the one place on a socket where C6 did not reach. */
  params?: PS;
  /** The upgrade's query string, validated before the world exists. */
  query?: QS;
  /** Schema for messages from the client. An invalid message is answered
   * with a `validation_failed` error envelope and dropped; the connection
   * stays open. */
  incoming?: I;
  /** Schema for messages to the client. */
  outgoing?: O;
  /** The OpenAPI `operationId`; the message schemas become
   * `components.schemas.<OperationId>Incoming` / `…Outgoing`. */
  operationId?: string;
  auth?: A;
  resources?: R;
}

type Out<S> = S extends AnySchema ? Output<S> : unknown;

/** The three moments of a connection.
 *
 * @category Streams and WebSockets
 */
export interface SocketHandlers<
  I,
  O,
  R = ResourceDeclaration[],
  A = unknown,
  D = undefined,
  P = Record<string, string>,
  Q = Record<string, string | string[]>,
> {
  /** After the upgrade. Note that **a message is not delivered until `open`
   * returns**: a socket either pushes or converses, not both (GUIDE §9). */
  open?(ctx: SocketContext<I, O, R, A, D, P, Q>): unknown;
  /** Once per incoming message, in order. */
  message?(ctx: SocketContext<I, O, R, A, D, P, Q>): unknown;
  /** After the connection closed, however it closed — including when `open`
   * itself failed, which is how a push loop normally ends (`ctx.send`
   * rejects with `client_gone` once the client is gone). This is the only
   * place to release what the connection held. */
  close?(ctx: SocketContext<I, O, R, A, D, P, Q>): unknown;
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
  PS extends AnySchema | undefined = undefined,
  QS extends AnySchema | undefined = undefined,
>(
  path: string,
  options: SocketOptions<I, O, R, A, PS, QS>,
  handlers: SocketHandlers<
    Out<I>,
    Out<O>,
    R,
    A extends AuthDeclaration<infer P> ? P : undefined,
    A,
    PS extends AnySchema ? Output<PS> : Record<string, string>,
    QS extends AnySchema ? Output<QS> : Record<string, string | string[]>
  >,
): Workload {
  const contracts: Workload["contracts"] = {};
  if (options.params) contracts.params = options.params;
  if (options.query) contracts.query = options.query;
  if (options.incoming) contracts.message = options.incoming;
  if (options.outgoing) contracts.response = { 200: options.outgoing };
  const policies: WorkloadPolicies = {};
  if (options.concurrency !== undefined) policies.concurrency = options.concurrency;
  if (options.maxBodyBytes !== undefined) policies.maxBodyBytes = options.maxBodyBytes;
  return {
    __usai: "workload",
    kind: "socket",
    name: path,
    ...(options.summary ? { summary: options.summary } : {}),
    ...(options.description ? { description: options.description } : {}),
    trigger: { path },
    contracts,
    errors: [],
    ...(options.operationId ? { operationId: options.operationId } : {}),
    ...(options.auth ? { auth: options.auth } : {}),
    resources: withAuthResources(options.resources, options.auth),
    dispatches: [],
    publishes: [],
    policies,
    handler: handlers as unknown as Workload["handler"],
  };
}
