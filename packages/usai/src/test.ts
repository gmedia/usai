// `usai/test` — run the same application model tests will meet in
// production (`GOAL.md` §36). The harness spawns the `usai` runtime for the
// project, talks HTTP to the application, and invokes tasks, cron ticks, and
// commands deterministically through the control surface: no wall clock, no
// external server.
//
//   import { testApp } from "usai/test";
//   const app = await testApp({ root: "." });
//   const res = await app.http.post("/users", { body: { name: "Ayu" } });
//   await app.task("send-receipt").invoke({ orderId: "o1" });
//   await app.cron("cleanup").run();
//   await app.close();

import { spawn, type ChildProcess } from "node:child_process";
import { createInterface } from "node:readline";

export interface TestAppOptions {
  /** Project root (directory with usai.config.ts / src/app.ts). Default: cwd. */
  root?: string;
  /** Path to the `usai` binary. Default: $USAI_BIN, then `usai` on PATH. */
  binary?: string;
  /** Environment for the runtime (merged over process.env). */
  env?: Record<string, string>;
  /** Milliseconds to wait for the runtime to announce itself. Default 60000. */
  startTimeoutMs?: number;
  /** Extra CLI arguments (e.g. ["--status"]). */
  args?: string[];
}

export interface TestResponse {
  status: number;
  headers: Record<string, string>;
  body: unknown;
  text: string;
}

export interface RequestOptions {
  body?: unknown;
  headers?: Record<string, string>;
  query?: Record<string, string | number | boolean>;
}

export interface WorkOutcome<T = unknown> {
  ok: boolean;
  value: T;
  error: { name: string; message: string; usai?: { code: string; status: number; details?: unknown } } | null;
  termination: unknown;
  durationMs: number;
  violations: Array<{ code: string; message: string }>;
  logs: Array<{ level: string; message: string }>;
}

export interface TestApp {
  readonly url: string;
  readonly controlUrl: string;
  readonly http: {
    get(path: string, options?: RequestOptions): Promise<TestResponse>;
    post(path: string, options?: RequestOptions): Promise<TestResponse>;
    put(path: string, options?: RequestOptions): Promise<TestResponse>;
    patch(path: string, options?: RequestOptions): Promise<TestResponse>;
    delete(path: string, options?: RequestOptions): Promise<TestResponse>;
    request(method: string, path: string, options?: RequestOptions): Promise<TestResponse>;
  };
  task(name: string): { invoke<T = unknown>(input?: unknown): Promise<WorkOutcome<T>> };
  cron(name: string): { run<T = unknown>(): Promise<WorkOutcome<T>> };
  command(name: string): { run<T = unknown>(args?: string[]): Promise<WorkOutcome<T>> };
  /** Runtime status JSON (`/status` on the control surface). */
  status(): Promise<Record<string, unknown>>;
  close(): Promise<void>;
}

export class UsaiTestError extends Error {
  readonly outcome: WorkOutcome | undefined;
  constructor(message: string, outcome?: WorkOutcome) {
    super(message);
    this.name = "UsaiTestError";
    this.outcome = outcome;
  }
}

export async function testApp(options: TestAppOptions = {}): Promise<TestApp> {
  const binary = options.binary ?? process.env["USAI_BIN"] ?? "usai";
  const root = options.root ?? process.cwd();
  const token = `test-${Math.random().toString(36).slice(2)}`;
  const args = ["--root", root, "run", "--port", "0", "--control", "127.0.0.1:0", "--announce", ...(options.args ?? [])];
  const child: ChildProcess = spawn(binary, args, {
    env: { ...process.env, USAI_CONTROL_TOKEN: token, RUST_LOG: process.env["RUST_LOG"] ?? "warn", ...(options.env ?? {}) },
    stdio: ["ignore", "pipe", "inherit"],
  });
  const announced = new Promise<{ app: string; control: string }>((resolve, reject) => {
    const timer = setTimeout(() => reject(new UsaiTestError(`usai did not start within ${options.startTimeoutMs ?? 60000} ms`)), options.startTimeoutMs ?? 60000);
    child.once("error", (error) => { clearTimeout(timer); reject(new UsaiTestError(`could not spawn ${binary}: ${error.message}`)); });
    child.once("exit", (code) => { clearTimeout(timer); reject(new UsaiTestError(`usai exited with code ${code} before announcing`)); });
    const lines = createInterface({ input: child.stdout! });
    lines.on("line", (line) => {
      if (!line.startsWith("{")) return;
      try {
        const parsed = JSON.parse(line) as { app?: string; control?: string | null };
        if (parsed.app && parsed.control) { clearTimeout(timer); resolve({ app: parsed.app, control: parsed.control }); }
      } catch { /* not ours */ }
    });
  });
  const { app: url, control: controlUrl } = await announced;

  const control = async (path: string, body?: unknown): Promise<unknown> => {
    const init: RequestInit = { method: body === undefined ? "GET" : "POST", headers: { authorization: `Bearer ${token}`, "content-type": "application/json" } };
    if (body !== undefined) init.body = JSON.stringify(body);
    const res = await fetch(`${controlUrl}${path}`, init);
    const json = (await res.json()) as unknown;
    if (!res.ok) throw new UsaiTestError(`control ${path}: ${res.status} ${JSON.stringify(json)}`);
    return json;
  };
  const invoke = async <T,>(payload: Record<string, unknown>): Promise<WorkOutcome<T>> => (await control("/invoke", payload)) as WorkOutcome<T>;

  const request = async (method: string, path: string, options: RequestOptions = {}): Promise<TestResponse> => {
    const target = new URL(path, url);
    for (const [k, v] of Object.entries(options.query ?? {})) target.searchParams.append(k, String(v));
    const headers: Record<string, string> = { ...(options.headers ?? {}) };
    let body: string | undefined;
    if (options.body !== undefined) {
      body = typeof options.body === "string" ? options.body : JSON.stringify(options.body);
      if (typeof options.body !== "string" && !Object.keys(headers).some((h) => h.toLowerCase() === "content-type")) headers["content-type"] = "application/json";
    }
    const init: RequestInit = { method, headers };
    if (body !== undefined) init.body = body;
    const res = await fetch(target, init);
    const text = await res.text();
    let parsed: unknown = text;
    const type = res.headers.get("content-type") ?? "";
    if (type.includes("json") && text.length > 0) {
      try { parsed = JSON.parse(text); } catch { parsed = text; }
    }
    const out: Record<string, string> = {};
    res.headers.forEach((v, k) => { out[k] = v; });
    return { status: res.status, headers: out, body: parsed, text };
  };

  let closed = false;
  return {
    url,
    controlUrl,
    http: {
      get: (p, o) => request("GET", p, o),
      post: (p, o) => request("POST", p, o),
      put: (p, o) => request("PUT", p, o),
      patch: (p, o) => request("PATCH", p, o),
      delete: (p, o) => request("DELETE", p, o),
      request,
    },
    task: (name) => ({ invoke: (input) => invoke({ kind: "task", name, input: input ?? null }) }),
    cron: (name) => ({ run: () => invoke({ kind: "cron", name }) }),
    command: (name) => ({ run: (args) => invoke({ kind: "command", name, args: args ?? [] }) }),
    status: async () => (await control("/status")) as Record<string, unknown>,
    close: async () => {
      if (closed) return;
      closed = true;
      const exited = new Promise<void>((resolve) => child.once("exit", () => resolve()));
      try { await control("/stop", {}); } catch { child.kill("SIGINT"); }
      const timer = setTimeout(() => child.kill("SIGKILL"), 15000);
      await exited;
      clearTimeout(timer);
    },
  };
}
