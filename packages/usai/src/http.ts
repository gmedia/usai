// HTTP workload declarations (`GOAL.md` §10–§15, ADR-0003, ADR-0004).

import type { AnySchema, Output } from "./schema.ts";
import type { AuthDeclaration, DeclaredError, HttpOptions, Method, Workload, WorkloadPolicies } from "./declarations.ts";
import type { BaseContext } from "./runtime/context.ts";

/** Explicit response: status, headers, and a body the runtime encodes.
 *
 * @category HTTP
 */
export interface HttpResponse<T = unknown> {
  readonly __usai: "response";
  readonly status: number;
  readonly headers: Record<string, string>;
  readonly body: T;
}

/** Raw response for the escape hatch: bytes or text, no contract.
 *
 * @category HTTP
 */
export interface RawResponse {
  readonly __usai: "raw-response";
  readonly status: number;
  readonly headers: Record<string, string>;
  readonly text?: string;
  readonly bytes?: Uint8Array;
}

/** What an HTTP handler may return: the body (encoded as JSON with status
 * 200, or the single declared `response` status), an explicit
 * {@link HttpResponse} from `http.response`/`http.created`/…, or a
 * {@link RawResponse}. Promises of any of these are awaited.
 *
 * @category HTTP
 */
export type HttpHandlerResult<T> = T | HttpResponse<T> | RawResponse | Promise<T | HttpResponse<T> | RawResponse>;

type OutputOf<S, Fallback> = S extends AnySchema ? Output<S> : Fallback;
type ResponseOf<O extends HttpOptions> = O["response"] extends AnySchema
  ? Output<O["response"]>
  : O["response"] extends Record<number, AnySchema>
    ? Output<O["response"][keyof O["response"]]>
    : unknown;

/** The context of one HTTP request, typed from the declared contracts:
 * `params`, `query`, `headers` and `body` carry the schemas' output types
 * (already validated at the boundary, before this world existed), `auth`
 * carries the principal the auth declaration resolved. Everything on
 * {@link BaseContext} is available too. The world lives for this request
 * only; nothing here survives the response.
 *
 * @category HTTP
 */
export interface HttpContext<O extends HttpOptions = HttpOptions> extends BaseContext {
  readonly method: Method;
  readonly path: string;
  readonly url: string;
  readonly params: OutputOf<O["params"], Record<string, string>>;
  readonly query: OutputOf<O["query"], Record<string, string | string[]>>;
  readonly headers: OutputOf<O["headers"], Record<string, string>>;
  readonly body: OutputOf<O["body"], unknown>;
  readonly auth: O["auth"] extends AuthDeclaration<infer P> ? P : undefined;
}

/** The context of a raw request (`http.raw`): no contracts, the exact
 * bytes on `request`. Read the body once.
 *
 * @category HTTP
 */
export interface RawContext extends BaseContext {
  readonly method: Method;
  readonly path: string;
  readonly url: string;
  readonly params: Record<string, string>;
  readonly query: Record<string, string | string[]>;
  readonly headers: Record<string, string>;
  readonly request: {
    bytes(): Promise<Uint8Array>;
    text(): Promise<string>;
    json(): Promise<unknown>;
  };
}

function normalizeResponse(response: HttpOptions["response"]): Record<number, AnySchema> | undefined {
  if (response === undefined) return undefined;
  if ("~standard" in response) return { 200: response as AnySchema };
  return response as Record<number, AnySchema>;
}

function declare<O extends HttpOptions>(method: Method, path: string, options: O, handler: (ctx: HttpContext<O>) => HttpHandlerResult<ResponseOf<O>>): Workload {
  const contracts: Workload["contracts"] = {};
  if (options.params) contracts.params = options.params;
  if (options.query) contracts.query = options.query;
  if (options.headers) contracts.headers = options.headers;
  if (options.body) contracts.body = options.body;
  const response = normalizeResponse(options.response);
  if (response) contracts.response = response;
  const policies: WorkloadPolicies = {};
  if (options.timeout !== undefined) policies.timeout = options.timeout;
  if (options.concurrency !== undefined) policies.concurrency = options.concurrency;
  return {
    __usai: "workload",
    kind: "http",
    name: `${method} ${path}`,
    trigger: { method, path, raw: false },
    contracts,
    errors: options.errors ?? [],
    ...(options.auth ? { auth: options.auth } : {}),
    resources: options.resources ?? [],
    dispatches: [],
    publishes: [],
    policies,
    handler: handler as Workload["handler"],
  };
}

/** The signature of `http.get`/`post`/…. */
export type Declare = <O extends HttpOptions>(path: string, options: O, handler: (ctx: HttpContext<O>) => HttpHandlerResult<ResponseOf<O>>) => Workload;

function method(m: Method): Declare {
  return (path, options, handler) => declare(m, path, options, handler);
}

/** Options for `http.raw`.
 *
 * @category HTTP
 */
export interface RawOptions extends WorkloadPolicies {
  /** HTTP method. Default `POST`. */
  method?: Method;
  auth?: AuthDeclaration;
  resources?: import("./declarations.ts").ResourceDeclaration[];
  /** Errors the handler answers with (documented in the reference). */
  errors?: DeclaredError[];
  /** Statuses the handler writes, with a description each — the reference
   * lists them instead of "opaque response". */
  responses?: Record<number, string>;
}

/** The handler of `http.raw`. */
export type RawHandler = (ctx: RawContext) => RawResponse | HttpResponse | Promise<RawResponse | HttpResponse>;

/** Low-level escape hatch: exact bytes in, raw response out. Schema
 * validation and generated docs are unavailable for this endpoint. */
function raw(path: string, options: RawOptions, handler: RawHandler): Workload;
function raw(path: string, handler: RawHandler): Workload;
function raw(path: string, a: RawOptions | RawHandler, b?: RawHandler): Workload {
  const options: RawOptions = typeof a === "function" ? {} : a;
  const handler = (typeof a === "function" ? a : b) as Workload["handler"];
  const m = options.method ?? "POST";
  const policies: WorkloadPolicies = {};
  if (options.timeout !== undefined) policies.timeout = options.timeout;
  if (options.concurrency !== undefined) policies.concurrency = options.concurrency;
  return {
    __usai: "workload",
    kind: "http",
    name: `${m} ${path}`,
    trigger: { method: m, path, raw: true, ...(options.responses ? { responses: options.responses } : {}) },
    contracts: {},
    errors: options.errors ?? [],
    ...(options.auth ? { auth: options.auth } : {}),
    resources: options.resources ?? [],
    dispatches: [],
    publishes: [],
    policies,
    handler,
  };
}

import { streams } from "./connection.ts";

/**
 * Declare HTTP endpoints. Each `http.<method>(path, options, handler)`
 * returns a {@link Workload} to list in `defineApp`/`defineModule`.
 *
 * A request is a **finite** unit of work: the runtime routes it, decodes
 * it and validates `params`/`query`/`headers`/`body` against the declared
 * schemas **before a world exists**, so a 400 never runs application
 * code. Then a fresh world runs the `auth` resolver (401 stops there) and
 * the handler with an {@link HttpContext}, bounded by `timeout` (the
 * runtime default when undeclared) and `concurrency`. When the handler returns,
 * the world ends: a pending `ctx.tasks.invoke`, an open transaction or an
 * un-awaited resource operation at that point is a lifecycle error, not a
 * silent drop (hand work off with `ctx.tasks.dispatch` instead).
 *
 * `response` is one schema (status 200) or a map of status to schema;
 * `errors` lists the `{ code, status }` pairs the handler throws so the
 * reference page and the OpenAPI document can say so.
 *
 * @example
 * ```ts
 * export const createUser = http.post(
 *   "/users",
 *   { body: NewUser, response: { 201: User }, auth: session, resources: [db], errors: [{ code: "conflict", status: 409 }] },
 *   async (ctx) => {
 *     const row = await ctx.resources.db.one<User>("insert into users … returning *", [ctx.body.name]);
 *     await ctx.tasks.dispatch(sendWelcome, { userId: row.id }); // outlives the response, explicitly
 *     return http.created(row);
 *   },
 * );
 * ```
 *
 * @category HTTP
 */
export const http = {
  /** Streaming response (`text/event-stream` by default): a
   * connection-bound world that lives until the handler returns; see
   * {@link StreamHandle}. */
  stream: streams.stream,
  /** `GET` endpoint. */
  get: method("GET"),
  /** `POST` endpoint. */
  post: method("POST"),
  /** `PUT` endpoint. */
  put: method("PUT"),
  /** `PATCH` endpoint. */
  patch: method("PATCH"),
  /** `DELETE` endpoint. */
  delete: method("DELETE"),
  /** `HEAD` endpoint. */
  head: method("HEAD"),
  /** `OPTIONS` endpoint. */
  options: method("OPTIONS"),

  /** Low-level escape hatch: exact bytes in ({@link RawContext}), a
   * {@link RawResponse} or {@link HttpResponse} out. No schema validation;
   * the reference shows the statuses from `responses`. */
  raw,

  /** An explicit status and headers around a contract-encoded body. */
  response<T>(status: number, body: T, headers: Record<string, string> = {}): HttpResponse<T> {
    return { __usai: "response", status, headers, body };
  },
  /** `201 Created` with a body. */
  created<T>(body: T, headers: Record<string, string> = {}): HttpResponse<T> {
    return { __usai: "response", status: 201, headers, body };
  },
  /** `202 Accepted` with a body: the work continues elsewhere (a dispatched task). */
  accepted<T>(body: T, headers: Record<string, string> = {}): HttpResponse<T> {
    return { __usai: "response", status: 202, headers, body };
  },
  /** `204 No Content`. */
  noContent(headers: Record<string, string> = {}): HttpResponse<null> {
    return { __usai: "response", status: 204, headers, body: null };
  },
  /** Raw text or bytes with an explicit status, for `http.raw` handlers. */
  rawResponse(status: number, body: string | Uint8Array, headers: Record<string, string> = {}): RawResponse {
    return typeof body === "string"
      ? { __usai: "raw-response", status, headers, text: body }
      : { __usai: "raw-response", status, headers, bytes: body };
  },
};

/** Whether a handler result is an explicit {@link HttpResponse}.
 *
 * @category HTTP
 */
export function isHttpResponse(value: unknown): value is HttpResponse {
  return typeof value === "object" && value !== null && (value as HttpResponse).__usai === "response";
}

/** Whether a handler result is a {@link RawResponse}.
 *
 * @category HTTP
 */
export function isRawResponse(value: unknown): value is RawResponse {
  return typeof value === "object" && value !== null && (value as RawResponse).__usai === "raw-response";
}
