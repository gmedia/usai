/**
 * `@sakaladev/usai/test` — run the same application model tests will
 * meet in production. The harness spawns the `usai` runtime for the
 * project, talks HTTP to the application, and invokes tasks, cron ticks
 * and commands deterministically through the control surface: no wall
 * clock, no external server. Start with {@link testApp}.
 *
 * @module
 */

import { spawn, type ChildProcess } from "node:child_process";
import { createInterface } from "node:readline";

/** Options for {@link testApp}.
 *
 * @category Testing
 */
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
  /** Run `usai db migrate` (and optionally `usai db seed`) against the
   * configured database before starting — for tests on a throwaway
   * database. Migrations are never run at startup by the runtime itself. */
  migrate?: boolean | { seed?: boolean | string };
}

/** One HTTP response from the application under test: `body` is the
 * parsed JSON when the response was JSON, otherwise the text.
 *
 * @category Testing
 */
export interface TestResponse {
  status: number;
  headers: Record<string, string>;
  body: unknown;
  text: string;
  /** The response body's exact bytes (a downloaded file, a binary raw response). */
  bytes: Uint8Array;
  /** Lifecycle violations the request's world committed (`detached_work`,
   * …), from the `x-usai-lifecycle` header the harness's runtime exposes.
   * A request that followed the rules has `[]`; assert on it. */
  violations: string[];
}

/** Options for `app.http.*`.
 *
 * @category Testing
 */
export interface RequestOptions {
  /** A JSON value (serialized, `content-type: application/json`), a raw
   * string, or bytes (`Uint8Array`, sent as-is — set `content-type` in
   * `headers`, `application/octet-stream` otherwise: a file upload to an
   * `http.raw` route). */
  body?: unknown;
  headers?: Record<string, string>;
  query?: Record<string, string | number | boolean>;
}

/** The result of a task, cron tick or command run through the control
 * surface: the handler's value or its error, the world's termination, the
 * lifecycle `violations` it committed (detached work, an open transaction)
 * and its log lines. A test asserts on all of it.
 *
 * @category Testing
 */
export interface WorkOutcome<T = unknown> {
  /** The handler returned (no throw, no violation). */
  ok: boolean;
  value: T;
  error: {
    name: string;
    message: string;
    usai?: { code: string; status: number; details?: unknown };
  } | null;
  termination: unknown;
  durationMs: number;
  violations: Array<{ code: string; message: string }>;
  logs: Array<{ level: string; message: string }>;
}

/** A running application under test; see {@link testApp}.
 *
 * @category Testing
 */
export interface TestApp {
  /** Base URL of the application listener. */
  readonly url: string;
  /** Base URL of the control surface. */
  readonly controlUrl: string;
  /** HTTP requests to the application, with JSON in and out. */
  readonly http: {
    get(path: string, options?: RequestOptions): Promise<TestResponse>;
    post(path: string, options?: RequestOptions): Promise<TestResponse>;
    put(path: string, options?: RequestOptions): Promise<TestResponse>;
    patch(path: string, options?: RequestOptions): Promise<TestResponse>;
    delete(path: string, options?: RequestOptions): Promise<TestResponse>;
    request(method: string, path: string, options?: RequestOptions): Promise<TestResponse>;
  };
  /** Run a task once in a fresh world and get its {@link WorkOutcome}. */
  task(name: string): { invoke<T = unknown>(input?: unknown): Promise<WorkOutcome<T>> };
  /** Run one cron tick, without the clock. */
  cron(name: string): { run<T = unknown>(): Promise<WorkOutcome<T>> };
  /** Run a command with arguments. */
  command(name: string): { run<T = unknown>(args?: string[]): Promise<WorkOutcome<T>> };
  /** Deliver one message to a topic's consumer directly — a fresh world,
   * `ctx.attempt` 1, no row in `usai_queue`, no retry: the way to test a
   * consumer's logic (idempotency: deliver the same message twice) without
   * publishing through the application or waiting for the scheduler. */
  queue(topic: string): { deliver<T = unknown>(message: unknown): Promise<WorkOutcome<T>> };
  /** Runtime status JSON (`/status` on the control surface). */
  status(): Promise<Record<string, unknown>>;
  /** Stop the runtime (drains, then exits). Always call it, in `after`. */
  close(): Promise<void>;
}

/** Thrown when the harness itself fails: the binary is missing, the
 * runtime did not start, a control request was refused.
 *
 * @category Testing
 */
export class UsaiTestError extends Error {
  readonly outcome: WorkOutcome | undefined;
  constructor(message: string, outcome?: WorkOutcome) {
    super(message);
    this.name = "UsaiTestError";
    this.outcome = outcome;
  }
}

function runCli(binary: string, args: string[], env: Record<string, string>): Promise<void> {
  return new Promise((resolve, reject) => {
    const child = spawn(binary, args, {
      env: { ...process.env, ...env },
      stdio: ["ignore", "ignore", "pipe"],
    });
    let stderr = "";
    child.stderr!.on("data", (chunk: Buffer) => {
      stderr += chunk.toString();
    });
    child.once("error", (error) =>
      reject(new UsaiTestError(`could not spawn ${binary}: ${error.message}`)),
    );
    child.once("exit", (code) =>
      code === 0
        ? resolve()
        : reject(new UsaiTestError(`usai ${args.join(" ")} exited with code ${code}\n${stderr}`)),
    );
  });
}

/**
 * Start the application under the real runtime for a test: the same
 * artifact, the same boundaries, the same worlds production will run.
 * The harness spawns the `usai` binary (`USAI_BIN`, `binary`, or `usai` on
 * PATH) on random ports with a control token, waits for it to announce
 * itself, and talks HTTP to the application and to the control surface.
 * Tasks, cron ticks and commands run deterministically through the
 * control surface — no wall clock, no external server — and answer with a
 * {@link WorkOutcome} that includes the world's lifecycle violations, so a
 * test can fail on detached work the way production would.
 *
 * `usai test` sets `USAI_BIN` and runs `node --test` over the project.
 *
 * @example
 * ```ts
 * import { test, before, after } from "node:test";
 * import assert from "node:assert/strict";
 * import { testApp, type TestApp } from "@sakaladev/usai/test";
 *
 * let app: TestApp;
 * before(async () => { app = await testApp({ root: ".", migrate: true }); });
 * after(() => app.close());
 *
 * test("creates a user and hands off the welcome mail", async () => {
 *   const res = await app.http.post("/users", { body: { name: "Ayu" } });
 *   assert.equal(res.status, 201);
 *   const mail = await app.task("send-welcome").invoke({ userId: (res.body as { id: string }).id });
 *   assert.ok(mail.ok, mail.error?.message);
 *   assert.deepEqual(mail.violations, []);
 * });
 * ```
 *
 * @category Testing
 */
export async function testApp(options: TestAppOptions = {}): Promise<TestApp> {
  const binary = options.binary ?? process.env["USAI_BIN"] ?? "usai";
  const root = options.root ?? process.cwd();
  if (options.migrate) {
    const env = { RUST_LOG: process.env["RUST_LOG"] ?? "warn", ...(options.env ?? {}) };
    await runCli(binary, ["--root", root, "db", "migrate"], env);
    const seed = typeof options.migrate === "object" ? options.migrate.seed : undefined;
    if (seed)
      await runCli(
        binary,
        ["--root", root, "db", "seed", ...(typeof seed === "string" ? [seed] : [])],
        env,
      );
  }
  const token = `test-${Math.random().toString(36).slice(2)}`;
  // --diagnostics: the runtime reports lifecycle violations and error details
  // to the client, which is what a test wants to assert on.
  const args = [
    "--root",
    root,
    "run",
    "--port",
    "0",
    "--control",
    "127.0.0.1:0",
    "--announce",
    "--diagnostics",
    ...(options.args ?? []),
  ];
  const child: ChildProcess = spawn(binary, args, {
    env: {
      ...process.env,
      USAI_CONTROL_TOKEN: token,
      // No load balancer in front of a test runtime: stop at once on SIGINT.
      USAI_DRAIN_GRACE: "0",
      RUST_LOG: process.env["RUST_LOG"] ?? "warn",
      ...(options.env ?? {}),
    },
    stdio: ["ignore", "pipe", "inherit"],
  });
  const announced = new Promise<{ app: string; control: string }>((resolve, reject) => {
    const timer = setTimeout(
      () =>
        reject(
          new UsaiTestError(`usai did not start within ${options.startTimeoutMs ?? 60000} ms`),
        ),
      options.startTimeoutMs ?? 60000,
    );
    child.once("error", (error) => {
      clearTimeout(timer);
      reject(new UsaiTestError(`could not spawn ${binary}: ${error.message}`));
    });
    child.once("exit", (code) => {
      clearTimeout(timer);
      // The runtime's own error is on stderr just above; add what a test author can do about the common one.
      reject(
        new UsaiTestError(
          `usai exited with code ${code} before announcing — if it reported a missing environment variable: testApp starts the runtime with the process environment plus \`env: {...}\`; \`usai test\` also loads .env, a plain \`node --test\` does not`,
        ),
      );
    });
    const lines = createInterface({ input: child.stdout! });
    lines.on("line", (line) => {
      if (!line.startsWith("{")) return;
      try {
        const parsed = JSON.parse(line) as { app?: string; control?: string | null };
        if (parsed.app && parsed.control) {
          clearTimeout(timer);
          resolve({ app: parsed.app, control: parsed.control });
        }
      } catch {
        /* not ours */
      }
    });
  });
  const { app: url, control: controlUrl } = await announced;

  const control = async (path: string, body?: unknown): Promise<unknown> => {
    const init: RequestInit = {
      method: body === undefined ? "GET" : "POST",
      headers: { authorization: `Bearer ${token}`, "content-type": "application/json" },
    };
    if (body !== undefined) init.body = JSON.stringify(body);
    const res = await fetch(`${controlUrl}${path}`, init);
    const json = (await res.json()) as unknown;
    if (!res.ok) throw new UsaiTestError(`control ${path}: ${res.status} ${JSON.stringify(json)}`);
    return json;
  };
  const invoke = async <T>(payload: Record<string, unknown>): Promise<WorkOutcome<T>> =>
    (await control("/invoke", payload)) as WorkOutcome<T>;

  const request = async (
    method: string,
    path: string,
    options: RequestOptions = {},
  ): Promise<TestResponse> => {
    const target = new URL(path, url);
    for (const [k, v] of Object.entries(options.query ?? {}))
      target.searchParams.append(k, String(v));
    const headers: Record<string, string> = { ...(options.headers ?? {}) };
    let body: string | Uint8Array | undefined;
    const hasContentType = Object.keys(headers).some((h) => h.toLowerCase() === "content-type");
    if (options.body instanceof Uint8Array) {
      body = options.body;
      if (!hasContentType) headers["content-type"] = "application/octet-stream";
    } else if (options.body !== undefined) {
      body = typeof options.body === "string" ? options.body : JSON.stringify(options.body);
      if (typeof options.body !== "string" && !hasContentType)
        headers["content-type"] = "application/json";
    }
    const init: RequestInit = { method, headers };
    if (body !== undefined)
      init.body =
        typeof body === "string"
          ? body
          : new Blob([Uint8Array.from(body) as Uint8Array<ArrayBuffer>]);
    const res = await fetch(target, init);
    const raw = new Uint8Array(await res.arrayBuffer());
    const text = new TextDecoder().decode(raw);
    let parsed: unknown = text;
    const type = res.headers.get("content-type") ?? "";
    if (type.includes("json") && text.length > 0) {
      try {
        parsed = JSON.parse(text);
      } catch {
        parsed = text;
      }
    }
    const out: Record<string, string> = {};
    res.headers.forEach((v, k) => {
      out[k] = v;
    });
    const violations = (out["x-usai-lifecycle"] ?? "")
      .split(",")
      .map((s) => s.trim())
      .filter(Boolean);
    return { status: res.status, headers: out, body: parsed, text, bytes: raw, violations };
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
    queue: (topic) => ({
      deliver: (message) => invoke({ kind: "queue", name: topic, input: message ?? null }),
    }),
    status: async () => (await control("/status")) as Record<string, unknown>,
    close: async () => {
      if (closed) return;
      closed = true;
      const exited = new Promise<void>((resolve) => child.once("exit", () => resolve()));
      try {
        await control("/stop", {});
      } catch {
        child.kill("SIGINT");
      }
      const timer = setTimeout(() => child.kill("SIGKILL"), 15000);
      await exited;
      clearTimeout(timer);
    },
  };
}
