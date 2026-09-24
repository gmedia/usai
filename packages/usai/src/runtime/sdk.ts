// The in-world dispatcher: `globalThis.__usai_sdk.invoke(app, index, input)`.
// Builds the typed context for the workload's kind, runs boundary parsing,
// invokes the handler, and encodes the outcome for the host.

import { type AppDeclaration, type Workload, flatten } from "../declarations.ts";
import { type Finalizer, REPARSE, hostFinal, prepareSchema, structuralSample } from "./prepare.ts";
import { UsaiError, isUsaiError } from "../errors.ts";
import { parseCookies } from "../cookies.ts";
import { bytes } from "../bytes.ts";
import { type ResponseHeaders, isHttpResponse, isRawResponse } from "../http.ts";
import { type AnySchema, validateWith } from "../schema.ts";
import { type BaseContext, makeBase, op, setRequestId } from "./context.ts";
import { describe } from "../manifest.ts";
import { resolveEnv } from "../env.ts";

interface HttpInput {
  kind: "http";
  env: Record<string, string | number | boolean | undefined>;
  request: {
    method: string;
    path: string;
    url: string;
    params: Record<string, string>;
    query: Record<string, string | string[]>;
    headers: Record<string, string>;
    body: { json?: unknown; text?: string; base64?: string } | null;
    /** The slots the host validated against their JSON Schema before the
     * world existed (C6). Absent when the host did not say. */
    validated?: string[];
  };
}

interface TaskInput {
  kind: "task";
  env: Record<string, string | number | boolean | undefined>;
  input: unknown;
  /** The id of the request whose world invoked or dispatched this task. */
  requestId?: string | null;
}
interface CronInput {
  kind: "cron";
  env: Record<string, string | number | boolean | undefined>;
  scheduledAt: string;
}
interface CommandInput {
  kind: "command";
  env: Record<string, string | number | boolean | undefined>;
  args: string[];
}
interface ServiceInput {
  kind: "service";
  env: Record<string, string | number | boolean | undefined>;
}
interface QueueInput {
  kind: "queue";
  env: Record<string, string | number | boolean | undefined>;
  message: unknown;
  id: string;
  attempt: number;
  /** The request id of the world that published the message, if any. */
  requestId?: string | null;
}
interface StreamInput {
  kind: "stream";
  env: Record<string, string | number | boolean | undefined>;
  request: HttpInput["request"];
}
interface SocketInput {
  kind: "socket";
  env: Record<string, string | number | boolean | undefined>;
  request: Omit<HttpInput["request"], "body">;
}
type Input =
  | HttpInput
  | TaskInput
  | CronInput
  | CommandInput
  | ServiceInput
  | QueueInput
  | StreamInput
  | SocketInput;

interface HttpOutput {
  status: number;
  headers: ResponseHeaders;
  json?: unknown;
  text?: string;
  base64?: string;
}

// A phase ledger for the attribution harness (`USAI_PROFILE=1`): the host
// seeds the world with profiling on, the SDK records where the time inside
// the world went — dispatch (from `invoke` to the handler), validation of
// each slot, the handler, the response contract — and the bridge ships it
// with the outcome. Off by default: `mark` is a no-op and nothing allocates.
let ledger: Array<[string, number]> | null = null;
const now = (): number => {
  const perf = (globalThis as { performance?: { now(): number } }).performance;
  return perf !== undefined ? perf.now() : Date.now();
};
function profiling(): boolean {
  const bridge = (globalThis as { __usai?: { profiling?: () => boolean } }).__usai;
  return bridge !== undefined && typeof bridge.profiling === "function" && bridge.profiling();
}
function mark(phase: string, since: number): void {
  if (ledger !== null) ledger.push([phase, now() - since]);
}
/** The ledger of the current invocation, taken once (the bridge calls it). */
export function takeProfile(): Array<[string, number]> {
  const out = ledger ?? [];
  ledger = null;
  return out;
}

function parse<S extends AnySchema>(slot: string, schema: S | undefined, value: unknown): unknown {
  if (!schema) return value;
  const t = ledger !== null ? now() : 0;
  const result = validateWith(schema, value);
  mark(`validate.${slot}`, t);
  if (!result.ok) {
    throw new UsaiError("validation_failed", 400, `${slot} failed validation`, {
      slot,
      issues: result.issues,
    });
  }
  return result.value;
}

// Validate once (ADR-0018 follow-up, `docs/LIFECYCLE-CONTRACTS.md` C6): when
// the host already validated a slot and the schema's output is provably its
// input (`hostFinal`), the guest applies the finalizer instead of parsing
// again. The proof is computed once per schema — at warm-up, so it lives in
// the image — and never per request.
const finalizers = new WeakMap<AnySchema, Finalizer | null>();
function finalizerOf(schema: AnySchema): Finalizer | null {
  let f = finalizers.get(schema);
  if (f === undefined) {
    f = hostFinal(schema) ?? null;
    finalizers.set(schema, f);
  }
  return f;
}
function parseValidated<S extends AnySchema>(
  slot: string,
  schema: S | undefined,
  value: unknown,
  validated: string[] | undefined,
): unknown {
  if (!schema || !validated || !validated.includes(slot)) return parse(slot, schema, value);
  const finalize = finalizerOf(schema);
  if (finalize === null) return parse(slot, schema, value);
  const t = ledger !== null ? now() : 0;
  const out = finalize(value);
  if (out === REPARSE) return parse(slot, schema, value);
  mark(`validate.${slot}`, t);
  return out;
}

// Bodies cross the boundary as base64: the core's native codec when it has
// one, the JavaScript path otherwise (`bytes` in ../bytes.ts).
const bytesFromBase64 = (b64: string): Uint8Array => bytes.fromBase64(b64);
const base64FromBytes = (data: Uint8Array): string => bytes.toBase64(data);

function decodeBody(body: HttpInput["request"]["body"]): unknown {
  if (body === null) return undefined;
  if (body.json !== undefined) return body.json;
  if (body.text !== undefined) return body.text;
  if (body.base64 !== undefined) return bytesFromBase64(body.base64);
  return undefined;
}

async function authenticate(
  workload: Workload,
  base: BaseContext,
  request: HttpInput["request"],
): Promise<unknown> {
  const declaration = workload.auth;
  if (!declaration) return undefined;
  const ctx = {
    ...base,
    request: {
      method: request.method,
      path: request.path,
      headers: request.headers,
      query: request.query,
    },
  };
  if (declaration.scheme === "custom") return declaration.resolve(ctx, undefined);
  if (declaration.scheme === "cookie") {
    const name = declaration.credential?.name ?? "";
    const value = parseCookies(request.headers["cookie"])[name];
    if (value === undefined || value === "")
      throw new UsaiError("unauthorized", 401, `missing ${name} cookie`);
    return declaration.resolve(ctx, value);
  }
  const header = declaration.header ?? "authorization";
  const raw = request.headers[header];
  if (raw === undefined || raw === "")
    throw new UsaiError("unauthorized", 401, `missing ${header} header`);
  let credential = raw;
  if (declaration.scheme === "bearer") {
    const match = /^Bearer\s+(.+)$/i.exec(raw);
    if (!match) throw new UsaiError("unauthorized", 401, "expected a Bearer token");
    credential = match[1]!.trim();
  }
  return declaration.resolve(ctx, credential);
}

function defaultStatus(workload: Workload): number {
  const declared = Object.keys(workload.contracts.response ?? {})
    .map(Number)
    .filter((s) => s >= 200 && s < 300)
    .sort();
  return declared[0] ?? 200;
}

function encodeHttp(workload: Workload, result: unknown): HttpOutput {
  if (isRawResponse(result)) {
    const out: HttpOutput = { status: result.status, headers: result.headers };
    if (result.text !== undefined) out.text = result.text;
    if (result.bytes !== undefined) out.base64 = base64FromBytes(result.bytes);
    return out;
  }
  let status: number;
  let headers: ResponseHeaders;
  let body: unknown;
  if (isHttpResponse(result)) {
    ({ status, headers, body } = result);
  } else {
    status =
      result === undefined || result === null
        ? workload.contracts.response
          ? defaultStatus(workload)
          : 204
        : defaultStatus(workload);
    headers = {};
    body = result;
  }
  const schema = workload.contracts.response?.[status];
  if (schema) {
    // The same exact shortcut as for input slots: a final schema's
    // finalizer returns what the parse would (checks run, undeclared keys
    // stripped) or REPARSE, in which case the parse produces the issues.
    const finalize = finalizerOf(schema);
    const fast = finalize === null ? REPARSE : finalize(body);
    if (fast !== REPARSE) {
      body = fast;
    } else {
      const checked = validateWith(schema, body);
      if (!checked.ok) {
        throw new UsaiError(
          "response_contract_violation",
          500,
          `response for status ${status} does not match its contract`,
          { issues: checked.issues },
        );
      }
      body = checked.value;
    }
  }
  // Statuses that carry no body by definition go out without one, whatever
  // the handler returned beside them (`http.notModified()` has `null`).
  if (status === 204 || status === 205 || status === 304 || body === undefined)
    return { status, headers };
  return { status, headers, json: body === undefined ? null : body };
}

async function runHttp(workload: Workload, input: HttpInput): Promise<HttpOutput> {
  let t = ledger !== null ? now() : 0;
  const base = makeBase(workload.resources, input.env);
  mark("context", t);
  const { request } = input;
  const raw = workload.trigger["raw"] === true;
  // No declared auth: no resolver, no await (one microtask hop fewer).
  t = ledger !== null ? now() : 0;
  setRequestId(request.headers["x-request-id"]);
  const auth = workload.auth ? await authenticate(workload, base, request) : undefined;
  if (workload.auth) mark("auth", t);
  if (raw) {
    const bytes =
      request.body?.base64 !== undefined ? bytesFromBase64(request.body.base64) : new Uint8Array(0);
    const text = () => new TextDecoder().decode(bytes);
    const ctx = {
      ...base,
      auth,
      method: request.method,
      path: request.path,
      url: request.url,
      requestId: request.headers["x-request-id"] ?? "",
      params: request.params,
      query: request.query,
      headers: request.headers,
      request: {
        bytes: async () => bytes,
        text: async () => text(),
        json: async () => JSON.parse(text()) as unknown,
      },
    };
    const result = await (workload.handler as (ctx: unknown) => unknown)(ctx);
    return encodeHttp(workload, result);
  }
  const ctx = {
    ...base,
    auth,
    method: request.method,
    path: request.path,
    url: request.url,
    requestId: request.headers["x-request-id"] ?? "",
    params: parseValidated("params", workload.contracts.params, request.params, request.validated),
    query: parseValidated("query", workload.contracts.query, request.query, request.validated),
    headers: parseValidated(
      "headers",
      workload.contracts.headers,
      request.headers,
      request.validated,
    ),
    // The host validated `body.json` (or null when there was none); only
    // that exact value may skip the guest parse.
    body:
      request.body?.json !== undefined
        ? parseValidated("body", workload.contracts.body, request.body.json, request.validated)
        : parse("body", workload.contracts.body, decodeBody(request.body)),
  };
  t = ledger !== null ? now() : 0;
  const result = await (workload.handler as (ctx: unknown) => unknown)(ctx);
  mark("handler", t);
  t = ledger !== null ? now() : 0;
  const output = encodeHttp(workload, result);
  mark("response", t);
  return output;
}

async function runTask(workload: Workload, input: TaskInput): Promise<unknown> {
  const base = makeBase(workload.resources, input.env);
  setRequestId(input.requestId ?? undefined);
  const parsed = parse("input", workload.contracts.input, input.input);
  return (workload.handler as (ctx: unknown) => unknown)({
    ...base,
    input: parsed,
    requestId: input.requestId ?? "",
  });
}

async function runCron(workload: Workload, input: CronInput): Promise<unknown> {
  const base = makeBase(workload.resources, input.env);
  return (workload.handler as (ctx: unknown) => unknown)({
    ...base,
    scheduledAt: input.scheduledAt,
  });
}

async function runCommand(workload: Workload, input: CommandInput): Promise<unknown> {
  const base = makeBase(workload.resources, input.env);
  return (workload.handler as (ctx: unknown) => unknown)({ ...base, args: input.args });
}

async function runQueue(workload: Workload, input: QueueInput): Promise<unknown> {
  const base = makeBase(workload.resources, input.env);
  setRequestId(input.requestId ?? undefined);
  const message = parse("message", workload.contracts.message, input.message);
  return (workload.handler as (ctx: unknown) => unknown)({
    ...base,
    message,
    id: input.id,
    attempt: input.attempt,
    requestId: input.requestId ?? "",
  });
}

async function runStream(workload: Workload, input: StreamInput): Promise<unknown> {
  const base = makeBase(workload.resources, input.env);
  const { request } = input;
  setRequestId(request.headers["x-request-id"]);
  const auth = await authenticate(workload, base, request);
  const ctx = {
    ...base,
    auth,
    method: request.method,
    path: request.path,
    url: request.url,
    requestId: request.headers["x-request-id"] ?? "",
    params: parseValidated("params", workload.contracts.params, request.params, request.validated),
    query: parseValidated("query", workload.contracts.query, request.query, request.validated),
    headers: request.headers,
  };
  const stream = {
    start: async (options?: { status?: number; headers?: Record<string, string> }) => {
      await op("stream.start", { status: options?.status ?? 200, headers: options?.headers ?? {} });
    },
    send: async (chunk: string | Uint8Array) => {
      if (typeof chunk === "string") await op("stream.send", { text: chunk });
      else await op("stream.send", { base64: base64FromBytes(chunk) });
    },
    event: async (
      name: string,
      data: unknown,
      options?: { id?: string | number; retry?: number },
    ) => {
      const declared = workload.contracts.events?.[name];
      if (declared) {
        const checked = validateWith(declared, data);
        if (!checked.ok)
          throw new UsaiError(
            "event_contract_violation",
            500,
            `event ${name} does not match its declared schema`,
            { event: name, issues: checked.issues },
          );
        data = checked.value;
      }
      const payload = typeof data === "string" ? data : JSON.stringify(data);
      // Multi-line data is several `data:` lines (the wire format), so a
      // string with newlines reaches the client intact.
      const lines = payload
        .split("\n")
        .map((l) => `data: ${l}`)
        .join("\n");
      const id =
        options?.id !== undefined ? `id: ${String(options.id).replace(/[\r\n]/g, "")}\n` : "";
      const retry =
        options?.retry !== undefined ? `retry: ${Math.max(0, Math.floor(options.retry))}\n` : "";
      await op("stream.send", { text: `${id}${retry}event: ${name}\n${lines}\n\n` });
    },
  };
  // If nothing was streamed, the return value is an ordinary response.
  const result = await (workload.handler as (ctx: unknown, stream: unknown) => unknown)(
    ctx,
    stream,
  );
  return encodeHttp(workload, result);
}

interface SocketEvent {
  type: "text" | "binary" | "close";
  data?: string;
  base64?: string;
  code?: number | null;
  reason?: string;
}

async function runSocket(workload: Workload, input: SocketInput): Promise<unknown> {
  const base = makeBase(workload.resources, input.env);
  const { request } = input;
  const handlers = workload.handler as unknown as {
    open?: (ctx: unknown) => unknown;
    message?: (ctx: unknown) => unknown;
    close?: (ctx: unknown) => unknown;
  };
  // Browsers cannot set headers on `new WebSocket(url)`; the convention is
  // `new WebSocket(url, ["bearer", token])` — the credential rides in
  // `Sec-WebSocket-Protocol` and the server echoes the `bearer` subprotocol.
  // The header-carrying request wins when both are present.
  let headers = request.headers;
  let protocol: string | undefined;
  const offered = headers["sec-websocket-protocol"];
  if (
    workload.auth &&
    offered &&
    headers[(workload.auth.header ?? "authorization").toLowerCase()] === undefined
  ) {
    const parts = offered.split(",").map((p) => p.trim());
    const at = parts.findIndex((p) => p.toLowerCase() === "bearer");
    if (at >= 0 && parts[at + 1]) {
      const header = workload.auth.header ?? "authorization";
      const value =
        header.toLowerCase() === "authorization" ? `Bearer ${parts[at + 1]}` : parts[at + 1];
      headers = { ...headers, [header.toLowerCase()]: value } as Record<string, string>;
      protocol = "bearer";
    }
  }
  setRequestId(headers["x-request-id"]);
  const auth = await authenticate(workload, base, { ...request, headers, body: null });
  // Auth passed: let the upgrade complete (a refusal above answers 401 instead).
  await op("socket.accept", protocol ? { protocol } : {});
  const outgoing = workload.contracts.response?.[200];
  const state: Record<string, unknown> = {};
  const ctx = {
    ...base,
    auth,
    path: request.path,
    url: request.url,
    requestId: request.headers["x-request-id"] ?? "",
    params: request.params,
    query: request.query,
    headers: request.headers,
    state,
    message: undefined as unknown,
    closeInfo: null as { code: number | null; reason: string } | null,
    send: async (message: unknown) => {
      const checked = outgoing ? parse("outgoing", outgoing, message) : message;
      await op("socket.send", {
        text: typeof checked === "string" ? checked : JSON.stringify(checked),
      });
    },
    close: async (reason?: string) => {
      await op("socket.close", { reason: reason ?? "" });
    },
  };
  // `close` is the only place an application can release what the
  // connection held — a presence row, a subscription, a counter — so it has
  // to run however the connection ended. It used to run only after the
  // receive loop, which an `open` that threw never reached: a realtime round
  // left 717 presence rows behind because the normal end of a push loop is
  // `ctx.send` rejecting with `client_gone` once the client is gone.
  let failure: unknown;
  if (handlers.open) {
    try {
      await handlers.open(ctx);
    } catch (error) {
      failure = error;
    }
  }
  const clientGone = (error: unknown): boolean =>
    isUsaiError(error) && (error as { usai?: { code?: string } }).usai?.code === "client_gone";
  // A handler that failed for a real reason stops serving this connection;
  // one that failed *because* the client left is the ordinary ending, and
  // its close event is already waiting to be read below.
  if (failure !== undefined && !clientGone(failure)) {
    await op("socket.close", { reason: "" }).catch(() => {});
  }
  for (;;) {
    const event = await op<SocketEvent>("socket.recv", "");
    if (!event || event.type === "close") {
      ctx.closeInfo = { code: event?.code ?? null, reason: event?.reason ?? "" };
      break;
    }
    let raw: unknown = event.type === "text" ? event.data : bytesFromBase64(event.base64 ?? "");
    if (workload.contracts.message && typeof raw === "string") {
      try {
        raw = JSON.parse(raw);
      } catch {
        /* validated below as a string */
      }
    }
    try {
      ctx.message = workload.contracts.message
        ? parse("message", workload.contracts.message, raw)
        : raw;
    } catch (error) {
      // A message that fails its contract is reported to the client and
      // dropped; the connection stays open. The error envelope is not
      // subject to the outgoing contract.
      const detail = (error as { usai?: { details?: unknown } }).usai?.details;
      await op("socket.send", {
        text: JSON.stringify({
          error: {
            code: "validation_failed",
            message: (error as Error).message,
            details: detail ?? null,
          },
        }),
      }).catch(() => {});
      continue;
    }
    // A connection whose handler has already failed is drained, not served:
    // the loop is only still here to learn how the connection ended.
    if (failure !== undefined) continue;
    if (handlers.message) await handlers.message(ctx);
  }
  if (handlers.close) {
    try {
      await handlers.close(ctx);
    } catch (error) {
      if (failure === undefined) failure = error;
    }
  }
  // The client leaving is not a failure to report; every other error is.
  if (failure !== undefined && !clientGone(failure)) throw failure;
  return null;
}

async function runService(workload: Workload, input: ServiceInput): Promise<unknown> {
  const base = makeBase(workload.resources, input.env);
  return (workload.handler as (ctx: unknown) => unknown)(base);
}

// The application is immutable once evaluated (the image is a snapshot of
// it), so its flattened workload list is computed once — at warm-up, in the
// snapshot — and never per request (C13: no per-world definition rebuilds).
const flattened = new WeakMap<AppDeclaration, ReturnType<typeof flatten>>();
function workloadsOf(app: AppDeclaration): ReturnType<typeof flatten> {
  let f = flattened.get(app);
  if (!f) {
    f = flatten(app);
    flattened.set(app, f);
  }
  return f;
}

export async function invoke(
  app: AppDeclaration,
  index: number,
  inputJson: string,
): Promise<unknown> {
  ledger = profiling() ? [] : null;
  const t0 = ledger !== null ? now() : 0;
  const entry = workloadsOf(app).workloads[index];
  if (!entry) throw new UsaiError("unknown_workload", 500, `no workload at index ${index}`);
  const input = JSON.parse(inputJson) as Input;
  mark("dispatch", t0);
  // ctx.env carries typed values when the application declared them;
  // the host already validated presence and shape at activation.
  if (app.env) {
    const t = ledger !== null ? now() : 0;
    input.env = resolveEnv(
      app.env,
      input.env as Record<string, string | undefined>,
    ) as unknown as Record<string, string | number | boolean | undefined>;
    mark("env", t);
  }
  const { workload } = entry;
  try {
    switch (input.kind) {
      case "http":
        return await runHttp(workload, input);
      case "task":
        return { value: (await runTask(workload, input)) ?? null };
      case "cron":
        return { value: (await runCron(workload, input)) ?? null };
      case "command":
        return { value: (await runCommand(workload, input)) ?? null };
      case "service":
        return { value: (await runService(workload, input)) ?? null };
      case "queue":
        return { value: (await runQueue(workload, input)) ?? null };
      case "stream":
        return await runStream(workload, input);
      case "socket":
        return { value: (await runSocket(workload, input)) ?? null };
      default:
        throw new UsaiError(
          "unknown_input_kind",
          500,
          `unsupported input kind ${(input as { kind: string }).kind}`,
        );
    }
  } catch (error) {
    if (isUsaiError(error)) throw error;
    throw error;
  }
}

/** Prepares every declared schema at definition time so that lazily built
 * validator state (zod 4 computes schema internals on first use) lands in
 * the pre-initialized image instead of being rebuilt by every world.
 * Structural only: no handler, transform, refinement or default runs.
 * Measured: the first zod parse in a fresh world cost ~1.6 ms, later ones
 * ~0.03 ms. Returns the number of schema nodes touched. */
export function warm(app: AppDeclaration): number {
  let touched = 0;
  const one = (s: AnySchema) => {
    touched += prepareSchema(s);
    // The accepting path, when the schema provably runs no application
    // code on it: one real parse in the snapshot instead of one per world.
    finalizerOf(s);
    const sample = structuralSample(s);
    if (sample) {
      const r = validateWith(s, sample.value);
      if (r.ok) touched += 1;
    }
  };
  for (const { workload } of workloadsOf(app).workloads) {
    const c = workload.contracts;
    for (const s of [c.params, c.query, c.headers, c.body, c.input, c.message]) if (s) one(s);
    for (const s of Object.values(c.response ?? {})) if (s) one(s);
  }
  return touched;
}

/** Installs the SDK on the guest global. Idempotent; the last SDK evaluated
 * in a bundle wins, which is the one the application imported. */
export function install(): void {
  globalThis.__usai_sdk = { invoke, describe, warm, takeProfile };
}
