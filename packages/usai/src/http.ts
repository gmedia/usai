// HTTP workload declarations (`GOAL.md` §10–§15, ADR-0003, ADR-0004).

import type { AnySchema, Output } from "./schema.ts";
import type { AuthDeclaration, HttpOptions, Method, Workload, WorkloadPolicies } from "./declarations.ts";
import type { BaseContext } from "./runtime/context.ts";

/** Explicit response: status, headers, and a body the runtime encodes. */
export interface HttpResponse<T = unknown> {
  readonly __usai: "response";
  readonly status: number;
  readonly headers: Record<string, string>;
  readonly body: T;
}

/** Raw response for the escape hatch: bytes or text, no contract. */
export interface RawResponse {
  readonly __usai: "raw-response";
  readonly status: number;
  readonly headers: Record<string, string>;
  readonly text?: string;
  readonly bytes?: Uint8Array;
}

export type HttpHandlerResult<T> = T | HttpResponse<T> | RawResponse | Promise<T | HttpResponse<T> | RawResponse>;

type OutputOf<S, Fallback> = S extends AnySchema ? Output<S> : Fallback;
type ResponseOf<O extends HttpOptions> = O["response"] extends AnySchema
  ? Output<O["response"]>
  : O["response"] extends Record<number, AnySchema>
    ? Output<O["response"][keyof O["response"]]>
    : unknown;

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

type Declare = <O extends HttpOptions>(path: string, options: O, handler: (ctx: HttpContext<O>) => HttpHandlerResult<ResponseOf<O>>) => Workload;

function method(m: Method): Declare {
  return (path, options, handler) => declare(m, path, options, handler);
}

export interface RawOptions extends WorkloadPolicies {
  method?: Method;
  auth?: AuthDeclaration;
  resources?: import("./declarations.ts").ResourceDeclaration[];
}

type RawHandler = (ctx: RawContext) => RawResponse | HttpResponse | Promise<RawResponse | HttpResponse>;

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
    trigger: { method: m, path, raw: true },
    contracts: {},
    errors: [],
    ...(options.auth ? { auth: options.auth } : {}),
    resources: options.resources ?? [],
    dispatches: [],
    publishes: [],
    policies,
    handler,
  };
}

import { streams } from "./connection.ts";

export const http = {
  /** Streaming response: the world lives until the handler returns. */
  stream: streams.stream,
  get: method("GET"),
  post: method("POST"),
  put: method("PUT"),
  patch: method("PATCH"),
  delete: method("DELETE"),
  head: method("HEAD"),
  options: method("OPTIONS"),

  raw,

  /** Explicit status/headers around a contract-encoded body. */
  response<T>(status: number, body: T, headers: Record<string, string> = {}): HttpResponse<T> {
    return { __usai: "response", status, headers, body };
  },
  created<T>(body: T, headers: Record<string, string> = {}): HttpResponse<T> {
    return { __usai: "response", status: 201, headers, body };
  },
  accepted<T>(body: T, headers: Record<string, string> = {}): HttpResponse<T> {
    return { __usai: "response", status: 202, headers, body };
  },
  noContent(headers: Record<string, string> = {}): HttpResponse<null> {
    return { __usai: "response", status: 204, headers, body: null };
  },
  /** Raw text/bytes response for the escape hatch. */
  rawResponse(status: number, body: string | Uint8Array, headers: Record<string, string> = {}): RawResponse {
    return typeof body === "string"
      ? { __usai: "raw-response", status, headers, text: body }
      : { __usai: "raw-response", status, headers, bytes: body };
  },
};

export function isHttpResponse(value: unknown): value is HttpResponse {
  return typeof value === "object" && value !== null && (value as HttpResponse).__usai === "response";
}

export function isRawResponse(value: unknown): value is RawResponse {
  return typeof value === "object" && value !== null && (value as RawResponse).__usai === "raw-response";
}
