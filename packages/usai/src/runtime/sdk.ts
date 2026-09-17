// The in-world dispatcher: `globalThis.__usai_sdk.invoke(app, index, input)`.
// Builds the typed context for the workload's kind, runs boundary parsing,
// invokes the handler, and encodes the outcome for the host.

import { type AppDeclaration, type Workload, flatten } from "../declarations.ts";
import { UsaiError, isUsaiError } from "../errors.ts";
import { isHttpResponse, isRawResponse } from "../http.ts";
import { type AnySchema, validateWith } from "../schema.ts";
import { type BaseContext, makeBase } from "./context.ts";
import { describe } from "../manifest.ts";
import { resolveEnv } from "../env.ts";

interface HttpInput {
  kind: "http";
  env: Record<string, string>;
  request: {
    method: string;
    path: string;
    url: string;
    params: Record<string, string>;
    query: Record<string, string | string[]>;
    headers: Record<string, string>;
    body: { json?: unknown; text?: string; base64?: string } | null;
  };
}

interface TaskInput { kind: "task"; env: Record<string, string>; input: unknown }
interface CronInput { kind: "cron"; env: Record<string, string>; scheduledAt: string }
interface CommandInput { kind: "command"; env: Record<string, string>; args: string[] }
interface ServiceInput { kind: "service"; env: Record<string, string> }
interface QueueInput { kind: "queue"; env: Record<string, string>; message: unknown; id: string; attempt: number }
type Input = HttpInput | TaskInput | CronInput | CommandInput | ServiceInput | QueueInput;

interface HttpOutput {
  status: number;
  headers: Record<string, string>;
  json?: unknown;
  text?: string;
  base64?: string;
}

function parse<S extends AnySchema>(slot: string, schema: S | undefined, value: unknown): unknown {
  if (!schema) return value;
  const result = validateWith(schema, value);
  if (!result.ok) {
    throw new UsaiError("validation_failed", 400, `${slot} failed validation`, { slot, issues: result.issues });
  }
  return result.value;
}

function bytesFromBase64(b64: string): Uint8Array {
  const bin = atob(b64);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}

function base64FromBytes(bytes: Uint8Array): string {
  let bin = "";
  for (let i = 0; i < bytes.length; i++) bin += String.fromCharCode(bytes[i]!);
  return btoa(bin);
}

function decodeBody(body: HttpInput["request"]["body"]): unknown {
  if (body === null) return undefined;
  if (body.json !== undefined) return body.json;
  if (body.text !== undefined) return body.text;
  if (body.base64 !== undefined) return bytesFromBase64(body.base64);
  return undefined;
}

async function authenticate(workload: Workload, base: BaseContext, request: HttpInput["request"]): Promise<unknown> {
  const declaration = workload.auth;
  if (!declaration) return undefined;
  const ctx = { ...base, request: { method: request.method, path: request.path, headers: request.headers, query: request.query } };
  if (declaration.scheme === "custom") return declaration.resolve(ctx, undefined);
  const header = declaration.header ?? "authorization";
  const raw = request.headers[header];
  if (raw === undefined || raw === "") throw new UsaiError("unauthorized", 401, `missing ${header} header`);
  let credential = raw;
  if (declaration.scheme === "bearer") {
    const match = /^Bearer\s+(.+)$/i.exec(raw);
    if (!match) throw new UsaiError("unauthorized", 401, "expected a Bearer token");
    credential = match[1]!.trim();
  }
  return declaration.resolve(ctx, credential);
}

function defaultStatus(workload: Workload): number {
  const declared = Object.keys(workload.contracts.response ?? {}).map(Number).filter((s) => s >= 200 && s < 300).sort();
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
  let headers: Record<string, string>;
  let body: unknown;
  if (isHttpResponse(result)) {
    ({ status, headers, body } = result);
  } else {
    status = result === undefined || result === null ? (workload.contracts.response ? defaultStatus(workload) : 204) : defaultStatus(workload);
    headers = {};
    body = result;
  }
  const schema = workload.contracts.response?.[status];
  if (schema) {
    const checked = validateWith(schema, body);
    if (!checked.ok) {
      throw new UsaiError("response_contract_violation", 500, `response for status ${status} does not match its contract`, { issues: checked.issues });
    }
    body = checked.value;
  }
  if (status === 204 || body === undefined) return { status, headers };
  return { status, headers, json: body === undefined ? null : body };
}

async function runHttp(workload: Workload, input: HttpInput): Promise<HttpOutput> {
  const base = makeBase(workload.resources, input.env);
  const { request } = input;
  const raw = workload.trigger["raw"] === true;
  const auth = await authenticate(workload, base, request);
  if (raw) {
    const bytes = request.body?.base64 !== undefined ? bytesFromBase64(request.body.base64) : new Uint8Array(0);
    const text = () => new TextDecoder().decode(bytes);
    const ctx = {
      ...base,
      auth,
      method: request.method,
      path: request.path,
      url: request.url,
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
    params: parse("params", workload.contracts.params, request.params),
    query: parse("query", workload.contracts.query, request.query),
    headers: parse("headers", workload.contracts.headers, request.headers),
    body: parse("body", workload.contracts.body, decodeBody(request.body)),
  };
  const result = await (workload.handler as (ctx: unknown) => unknown)(ctx);
  return encodeHttp(workload, result);
}

async function runTask(workload: Workload, input: TaskInput): Promise<unknown> {
  const base = makeBase(workload.resources, input.env);
  const parsed = parse("input", workload.contracts.input, input.input);
  return (workload.handler as (ctx: unknown) => unknown)({ ...base, input: parsed });
}

async function runCron(workload: Workload, input: CronInput): Promise<unknown> {
  const base = makeBase(workload.resources, input.env);
  return (workload.handler as (ctx: unknown) => unknown)({ ...base, scheduledAt: input.scheduledAt });
}

async function runCommand(workload: Workload, input: CommandInput): Promise<unknown> {
  const base = makeBase(workload.resources, input.env);
  return (workload.handler as (ctx: unknown) => unknown)({ ...base, args: input.args });
}

async function runQueue(workload: Workload, input: QueueInput): Promise<unknown> {
  const base = makeBase(workload.resources, input.env);
  const message = parse("message", workload.contracts.message, input.message);
  return (workload.handler as (ctx: unknown) => unknown)({ ...base, message, id: input.id, attempt: input.attempt });
}

async function runService(workload: Workload, input: ServiceInput): Promise<unknown> {
  const base = makeBase(workload.resources, input.env);
  return (workload.handler as (ctx: unknown) => unknown)(base);
}

export async function invoke(app: AppDeclaration, index: number, inputJson: string): Promise<unknown> {
  const entry = flatten(app).workloads[index];
  if (!entry) throw new UsaiError("unknown_workload", 500, `no workload at index ${index}`);
  const input = JSON.parse(inputJson) as Input;
  // ctx.env carries typed values when the application declared them;
  // the host already validated presence and shape at activation.
  if (app.env) input.env = resolveEnv(app.env, input.env) as unknown as Record<string, string>;
  const { workload } = entry;
  try {
    switch (input.kind) {
      case "http": return await runHttp(workload, input);
      case "task": return { value: await runTask(workload, input) ?? null };
      case "cron": return { value: await runCron(workload, input) ?? null };
      case "command": return { value: await runCommand(workload, input) ?? null };
      case "service": return { value: await runService(workload, input) ?? null };
      case "queue": return { value: await runQueue(workload, input) ?? null };
      default: throw new UsaiError("unknown_input_kind", 500, `unsupported input kind ${(input as { kind: string }).kind}`);
    }
  } catch (error) {
    if (isUsaiError(error)) throw error;
    throw error;
  }
}

/** Installs the SDK on the guest global. Idempotent; the last SDK evaluated
 * in a bundle wins, which is the one the application imported. */
export function install(): void {
  globalThis.__usai_sdk = { invoke, describe };
}
