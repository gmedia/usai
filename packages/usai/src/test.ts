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
  /** Run the background schedulers: cron, queue consumers and services.
   *
   * **Off by default**, because a test run is supposed to be deterministic
   * and they are not: a `* * * * *` schedule fires in the middle of a test
   * from the wall clock, and — since `usai test` runs test files in
   * parallel against one database — two files' consumers take each other's
   * queue messages, so a retry test sees four attempts and its neighbour
   * sees two. `app.cron(name).run()`, `app.queue(topic).deliver(msg)` and
   * `app.task(name).invoke()` drive that work explicitly instead, one
   * delivery at a time.
   *
   * Turn them on for the tests that need the real scheduler — retry,
   * backoff and dead-lettering end to end — and give those a database of
   * their own (`env: { DATABASE_URL }`). */
  schedulers?: boolean | { cron?: boolean; queue?: boolean; services?: boolean };
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
   * A request that followed the rules has `[]`; assert on it.
   *
   * **Codes only**, because a response header is all a client gets. The
   * other `violations` — {@link WorkOutcome.violations}, from `invoke` and
   * `run` — is `{ code, message }[]`: there the harness holds the world's
   * own result, and a violation's message *is* its explanation. */
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
  /** How the world ended: `"completed"`, `"deadline-exceeded"`,
   * `"cancelled: <reason>"` or `"faulted: <detail>"`. This is what a
   * lifecycle test asserts on, and it used to be `unknown` — so asserting on
   * it needed a cast, which is the defect this release fixed for
   * `ctx.tasks.invoke`. The string form is open on purpose: a new
   * termination must not fail to typecheck in an existing test. */
  termination: Termination;
  durationMs: number;
  /** The world's lifecycle violations, with the message that explains each
   * one. The response-side {@link TestResponse.violations} is `string[]` —
   * codes only — because a response header is all a client gets. */
  violations: Array<{ code: string; message: string }>;
  /** Everything the world logged: the parsed `fields`, the workload and the
   * request id included, so a test that has the outcome does not have to go
   * back to `app.logs()` for them. (`app.logs()` adds the timestamp and the
   * runtime's own lines; this is only what the world wrote.) */
  logs: WorldLogLine[];
}

/** One line a world wrote, as the outcome carries it. */
export interface WorldLogLine {
  level: string;
  message: string;
  /** The trailing object of `ctx.log.info("paid", { id })`, parsed. */
  fields: Record<string, unknown> | null;
  workload: string;
  world: string;
  /** The request the world ran under, when it had one. */
  requestId: string | null;
}

/** How a world ended. The four the runtime names, plus the open form so a
 * future one is a value and not a type error. */
export type Termination = "completed" | "deadline-exceeded" | (string & {});

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
  /** Opens an event stream (`http.stream`, `text/event-stream`) and reads
   * it event by event: `const s = await app.stream("/events"); const first =
   * await s.next(); s.close()`. Headers (a cookie, a bearer token,
   * `last-event-id`) go in `options.headers`. The response's status and
   * headers are known once the promise resolves; a non-2xx status resolves
   * too (read `status`/`text` instead of `next`). */
  stream(path: string, options?: RequestOptions): Promise<TestStream>;
  /** Opens a WebSocket (`socket(...)`) with the headers a browser would send —
   * a `cookie`, or `["bearer", token]` as `protocols` — and exchanges JSON
   * messages: `const ws = await app.socket("/chat", { headers: { cookie } });
   * await ws.send({ text: "hi" }); const reply = await ws.next(); await ws.close()`.
   * A refused credential rejects with the HTTP status (`TestSocketRefused`,
   * `status` 401) before any frame. */
  socket(path: string, options?: SocketOptions): Promise<TestSocket>;
  /** Runtime status JSON (`/status` on the control surface). */
  status(): Promise<Record<string, unknown>>;
  /** The runtime's log lines so far (the last 10 000), newest last —
   * everything the application wrote with `console.*`/`ctx.log.*` (target
   * `app`, at INFO) and the runtime's own WARN/ERROR lines. Filter by the
   * request id a response carried (`res.headers["x-request-id"]`) to see
   * what a request did, **including the tasks it dispatched**: the id
   * follows the hand-off. */
  logs(filter?: LogFilter): LogLine[];
  /** Waits for a log line matching `filter` (already written or arriving
   * within `timeoutMs`, default 5 000) — how a test observes a dispatched
   * task, which runs after the response was sent. Rejects on timeout with
   * the lines seen so far. */
  waitForLog(filter: LogFilter, timeoutMs?: number): Promise<LogLine>;
  /** Stop the runtime (drains, then exits). Always call it, in `after`. */
  close(): Promise<void>;
}

/** One line of the runtime's JSON log, as `TestApp.logs` returns it.
 *
 * @category Testing
 */
export interface LogLine {
  timestamp: string;
  level: "TRACE" | "DEBUG" | "INFO" | "WARN" | "ERROR";
  message: string;
  /** `app` for the application's own lines; the runtime module otherwise. */
  target: string;
  /** The workload id (`task:send-reset-email`) when a world wrote the line. */
  workload?: string;
  world?: string;
  /** The request id the world ran under — the request's own, or the one
   * handed to a task it invoked or dispatched. */
  requestId?: string;
  /** Structured fields: the trailing object of `ctx.log.info("paid", { id })`. */
  fields?: Record<string, unknown>;
  /** Every other attribute the line carried. */
  [key: string]: unknown;
}

/** One server-sent event as `TestStream.next` returns it.
 *
 * @category Testing
 */
export interface SseEvent {
  /** The `event:` name; `"message"` when the frame had none. */
  event: string;
  /** The `data:` lines joined with `\n`. */
  data: string;
  /** `data` parsed as JSON when it is JSON, else `undefined`. */
  json: unknown;
  id?: string;
  retry?: number;
}

/** An open event stream.
 *
 * @category Testing
 */
export interface TestStream {
  readonly status: number;
  readonly headers: Record<string, string>;
  /** The next event, or `null` when the stream ended; rejects after `timeoutMs` (default 5 000). */
  next(timeoutMs?: number): Promise<SseEvent | null>;
  /** The whole body as text, for a stream that is not SSE (a CSV download). Waits for the end. */
  text(): Promise<string>;
  /** Closes the connection — what a browser does when the tab goes; the world is cancelled. */
  close(): void;
}

/** Options for `TestApp.socket`.
 *
 * @category Testing
 */
export interface SocketOptions {
  /** Request headers for the upgrade (`cookie`, …). */
  headers?: Record<string, string>;
  /** `Sec-WebSocket-Protocol` entries: `["bearer", token]` is how a browser passes a bearer token. */
  protocols?: readonly string[];
}

/** An open WebSocket.
 *
 * @category Testing
 */
export interface TestSocket {
  /** The subprotocol the server chose, if any. */
  readonly protocol: string | undefined;
  /** Sends one text frame: a string as is, anything else as JSON. */
  send(message: unknown): Promise<void>;
  /** The next text frame (parsed as JSON when it is JSON, else the string), or `null` once closed; rejects after `timeoutMs` (default 5 000). */
  next<T = unknown>(timeoutMs?: number): Promise<T | null>;
  /** Waits for the server's close frame: `{ code, reason }`. */
  closed(timeoutMs?: number): Promise<{ code: number; reason: string }>;
  /** Sends a close frame and ends the connection. */
  close(code?: number, reason?: string): Promise<void>;
}

/** Thrown by `TestApp.socket` when the server answered the upgrade with an
 * HTTP status instead of `101` (a refused credential is a `401`).
 *
 * @category Testing
 */
export class TestSocketRefused extends Error {
  readonly status: number;
  readonly body: string;
  constructor(status: number, body: string) {
    super(`websocket upgrade refused: ${status} ${body}`.trim());
    this.status = status;
    this.body = body;
  }
}

/** What `TestApp.logs` / `waitForLog` select on; every given field must match.
 *
 * @category Testing
 */
export interface LogFilter {
  /** Written as `res.headers["x-request-id"]`, which is `string | undefined`
   * — so the type has to admit `undefined` explicitly, or the idiom this
   * comment recommends does not compile under `exactOptionalPropertyTypes`
   * (which `tsconfig.base.json` here sets, and every example inherits). */
  requestId?: string | undefined;
  workload?: string | undefined;
  level?: LogLine["level"] | undefined;
  /** `app` for the application's lines. */
  target?: string | undefined;
  /** A substring of the message, or a pattern. */
  message?: string | RegExp | undefined;
  /** Any predicate over the parsed line. */
  where?: ((line: LogLine) => boolean) | undefined;
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
      stdio: ["ignore", "pipe", "pipe"],
    });
    // Both streams: a command that failed before it could log writes its
    // reason wherever it got to, and `exited with code 1` on its own names
    // nothing a test author can act on.
    let stderr = "";
    child.stderr!.on("data", (chunk: Buffer) => {
      stderr += chunk.toString();
    });
    child.stdout!.on("data", (chunk: Buffer) => {
      stderr += chunk.toString();
    });
    child.once("error", (error) =>
      reject(new UsaiTestError(`could not spawn ${binary}: ${error.message}`)),
    );
    child.once("exit", (code) =>
      code === 0
        ? resolve()
        : reject(
            new UsaiTestError(
              `usai ${args.join(" ")} exited with code ${code}\n${
                stderr.trim() === "" ? "  (it printed nothing)" : stderr
              }`,
            ),
          ),
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
  const schedulers =
    options.schedulers === true
      ? { cron: true, queue: true, services: true }
      : options.schedulers === undefined || options.schedulers === false
        ? { cron: false, queue: false, services: false }
        : {
            cron: options.schedulers.cron ?? false,
            queue: options.schedulers.queue ?? false,
            services: options.schedulers.services ?? false,
          };
  const args = [
    "--log-format",
    "json",
    "--root",
    root,
    "run",
    "--port",
    "0",
    "--control",
    "127.0.0.1:0",
    "--announce",
    "--diagnostics",
    ...(schedulers.cron ? [] : ["--no-cron"]),
    ...(schedulers.queue ? [] : ["--no-queue"]),
    ...(schedulers.services ? [] : ["--no-services"]),
    ...(options.args ?? []),
  ];
  const child: ChildProcess = spawn(binary, args, {
    env: {
      ...process.env,
      USAI_CONTROL_TOKEN: token,
      // No load balancer in front of a test runtime: stop at once on SIGINT.
      USAI_DRAIN_GRACE: "0",
      // `close()` covers the happy path; a CI cancel or a `kill -9` on the
      // test runner does not call it, and the runtime it started would go on
      // serving — holding its database pool — until the machine is rebooted.
      USAI_EXIT_WITH_PARENT: "1",
      // The runtime's own lines at WARN; the application's at INFO so
      // `app.logs()` sees what the handlers wrote.
      RUST_LOG: process.env["RUST_LOG"] ?? "warn,app=info",
      ...(options.env ?? {}),
    },
    stdio: ["ignore", "pipe", "pipe"],
  });
  // Every log line is kept (bounded) for `logs()` and forwarded to this
  // process's stderr as before, so a failing run still shows the runtime's
  // words in the terminal.
  const kept: LogLine[] = [];
  const waiters = new Set<(line: LogLine) => void>();
  // What reaches the terminal: the application's own lines (a `console.log`
  // in a handler is a debugging tool and must show), and anything the
  // runtime says at WARN or above. Its INFO narration is kept for
  // `app.logs()` but not printed — it was a wall of JSON between every two
  // test results. `USAI_TEST_LOGS=all` prints everything; `=none` prints
  // nothing.
  const forward = process.env["USAI_TEST_LOGS"] ?? "problems";
  createInterface({ input: child.stderr! }).on("line", (raw) => {
    if (!raw.startsWith("{")) {
      if (forward !== "none") process.stderr.write(`${raw}\n`);
      return;
    }
    let line: LogLine;
    try {
      const parsed = JSON.parse(raw) as Record<string, unknown>;
      line = normalizeLogLine(parsed);
    } catch {
      if (forward !== "none") process.stderr.write(`${raw}\n`);
      return;
    }
    const interesting =
      forward === "all" ||
      (forward !== "none" &&
        (line.target === "app" || line.level === "WARN" || line.level === "ERROR"));
    if (interesting) process.stderr.write(`${raw}\n`);
    kept.push(line);
    if (kept.length > 10_000) kept.splice(0, kept.length - 10_000);
    for (const waiter of waiters) waiter(line);
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

  /** Why a line that should exist never arrived, when the reason is that the
   * thing that writes it is switched off.
   *
   * The schedulers are off by default — a test run has to be deterministic —
   * and a suite that waits for `message dead-lettered` or a cron tick then
   * fails with `0 lines seen` and nothing else. The line it wants is written
   * by a consumer that was never started; without knowing that, the search
   * goes to the application. Saying so costs one sentence.
   */
  const schedulerHint = (
    filter: LogFilter,
    on: { cron: boolean; queue: boolean; services: boolean },
  ): string => {
    const text = `${filter.message ?? ""} ${filter.workload ?? ""}`.toLowerCase();
    const off: string[] = [];
    if (!on.queue && /queue|message|dead-letter|topic|consumer|retry/.test(text)) off.push("queue");
    if (!on.cron && /cron|tick|schedule/.test(text)) off.push("cron");
    if (!on.services && /service/.test(text)) off.push("services");
    if (off.length === 0) return "";
    const which = off.map((k) => `${k}: true`).join(", ");
    return `.\n  hint: testApp runs no background schedulers by default, so nothing writes this line. Pass \`testApp({ schedulers: { ${which} } })\` (and give the test a database of its own), or drive the work explicitly with app.queue().deliver() / app.cron().run()`;
  };

  const logs = (filter: LogFilter = {}): LogLine[] => kept.filter((l) => matches(l, filter));
  const waitForLog = (filter: LogFilter, timeoutMs = 5000): Promise<LogLine> => {
    const found = kept.find((l) => matches(l, filter));
    if (found) return Promise.resolve(found);
    return new Promise((resolve, reject) => {
      const waiter = (line: LogLine) => {
        if (matches(line, filter)) {
          waiters.delete(waiter);
          clearTimeout(timer);
          resolve(line);
        }
      };
      const timer = setTimeout(() => {
        waiters.delete(waiter);
        reject(
          new UsaiTestError(
            `no log line matched ${describeFilter(filter)} within ${timeoutMs} ms; ${kept.length} lines seen${kept.length ? `, the last: ${JSON.stringify(kept[kept.length - 1])}` : ""}${schedulerHint(filter, schedulers)}`,
          ),
        );
      }, timeoutMs);
      waiters.add(waiter);
    });
  };
  let closed = false;
  return {
    url,
    controlUrl,
    logs,
    waitForLog,
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
    stream: (path, options) => openStream(new URL(path, url), options),
    socket: (path, options) => openSocket(new URL(path, url), options ?? {}),
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

function normalizeLogLine(parsed: Record<string, unknown>): LogLine {
  const { request_id, fields, ...rest } = parsed;
  const line = rest as unknown as LogLine;
  if (typeof request_id === "string" && request_id.length > 0) line.requestId = request_id;
  if (typeof fields === "string") {
    try {
      line.fields = JSON.parse(fields) as Record<string, unknown>;
    } catch {
      line.fields = { raw: fields };
    }
  } else if (fields && typeof fields === "object") {
    line.fields = fields as Record<string, unknown>;
  }
  return line;
}

function matches(line: LogLine, filter: LogFilter): boolean {
  if (filter.requestId !== undefined && line.requestId !== filter.requestId) return false;
  if (filter.workload !== undefined && line.workload !== filter.workload) return false;
  if (filter.level !== undefined && line.level !== filter.level) return false;
  if (filter.target !== undefined && line.target !== filter.target) return false;
  if (filter.message !== undefined) {
    if (typeof filter.message === "string") {
      if (!line.message.includes(filter.message)) return false;
    } else if (!filter.message.test(line.message)) return false;
  }
  if (filter.where && !filter.where(line)) return false;
  return true;
}

function describeFilter(filter: LogFilter): string {
  const parts = Object.entries(filter)
    .filter(([, v]) => v !== undefined)
    .map(([k, v]) => `${k}=${typeof v === "function" ? "<predicate>" : String(v)}`);
  return parts.length ? parts.join(" ") : "<any line>";
}

async function openStream(target: URL, options: RequestOptions = {}): Promise<TestStream> {
  for (const [k, v] of Object.entries(options.query ?? {}))
    target.searchParams.append(k, String(v));
  const controller = new AbortController();
  const res = await fetch(target, {
    headers: { accept: "text/event-stream", ...(options.headers ?? {}) },
    signal: controller.signal,
  });
  const headers: Record<string, string> = {};
  res.headers.forEach((v, k) => {
    headers[k] = v;
  });
  const reader = res.body?.getReader();
  const decoder = new TextDecoder();
  let buffer = "";
  let ended = !reader;
  const pending: SseEvent[] = [];
  const parseFrame = (frame: string): SseEvent | null => {
    let event = "message";
    const data: string[] = [];
    let id: string | undefined;
    let retry: number | undefined;
    for (const line of frame.split("\n")) {
      if (line === "" || line.startsWith(":")) continue;
      const colon = line.indexOf(":");
      const field = colon < 0 ? line : line.slice(0, colon);
      const value = colon < 0 ? "" : line.slice(colon + 1).replace(/^ /, "");
      if (field === "event") event = value;
      else if (field === "data") data.push(value);
      else if (field === "id") id = value;
      else if (field === "retry" && /^\d+$/.test(value)) retry = Number(value);
    }
    if (data.length === 0 && id === undefined && retry === undefined) return null;
    const joined = data.join("\n");
    let json: unknown;
    try {
      json = JSON.parse(joined);
    } catch {
      json = undefined;
    }
    const out: SseEvent = { event, data: joined, json };
    if (id !== undefined) out.id = id;
    if (retry !== undefined) out.retry = retry;
    return out;
  };
  const pull = async (): Promise<boolean> => {
    if (!reader) return false;
    const { value, done } = await reader.read();
    if (done) {
      ended = true;
      return false;
    }
    buffer += decoder.decode(value, { stream: true });
    let at: number;
    while ((at = buffer.indexOf("\n\n")) >= 0) {
      const frame = buffer.slice(0, at);
      buffer = buffer.slice(at + 2);
      const parsed = parseFrame(frame.replace(/\r/g, ""));
      if (parsed) pending.push(parsed);
    }
    return true;
  };
  const next = async (timeoutMs = 5000): Promise<SseEvent | null> => {
    const deadline = Date.now() + timeoutMs;
    while (pending.length === 0) {
      if (ended) return null;
      const remaining = deadline - Date.now();
      if (remaining <= 0)
        throw new UsaiTestError(`no event within ${timeoutMs} ms on ${target.pathname}`);
      const more = await Promise.race([
        pull(),
        new Promise<"timeout">((r) => setTimeout(() => r("timeout"), remaining)),
      ]);
      if (more === "timeout")
        throw new UsaiTestError(`no event within ${timeoutMs} ms on ${target.pathname}`);
    }
    return pending.shift() ?? null;
  };
  const text = async (): Promise<string> => {
    const parts: string[] = [];
    if (buffer) parts.push(buffer);
    while (reader && !ended) {
      const { value, done } = await reader.read();
      if (done) break;
      parts.push(decoder.decode(value, { stream: true }));
    }
    ended = true;
    return parts.join("");
  };
  return {
    status: res.status,
    headers,
    next,
    text,
    close: () => {
      controller.abort();
      ended = true;
    },
  };
}

async function openSocket(target: URL, options: SocketOptions): Promise<TestSocket> {
  const net = await import("node:net");
  const { createHash, randomBytes } = await import("node:crypto");
  const key = randomBytes(16).toString("base64");
  const port = Number(target.port || 80);
  const socket = net.connect(port, target.hostname);
  await new Promise<void>((resolve, reject) => {
    socket.once("connect", () => resolve());
    socket.once("error", reject);
  });
  const lines = [
    `GET ${target.pathname}${target.search} HTTP/1.1`,
    `Host: ${target.host}`,
    "Upgrade: websocket",
    "Connection: Upgrade",
    `Sec-WebSocket-Key: ${key}`,
    "Sec-WebSocket-Version: 13",
  ];
  if (options.protocols?.length)
    lines.push(`Sec-WebSocket-Protocol: ${options.protocols.join(", ")}`);
  for (const [k, v] of Object.entries(options.headers ?? {})) lines.push(`${k}: ${v}`);
  socket.write(`${lines.join("\r\n")}\r\n\r\n`);
  // The upgrade response, then frames.
  let buffer = Buffer.alloc(0);
  const chunks: Array<() => void> = [];
  let ended = false;
  socket.on("data", (d: Buffer) => {
    buffer = Buffer.concat([buffer, d]);
    for (const wake of chunks.splice(0)) wake();
  });
  socket.on("close", () => {
    ended = true;
    for (const wake of chunks.splice(0)) wake();
  });
  socket.on("error", () => {
    ended = true;
    for (const wake of chunks.splice(0)) wake();
  });
  const waitData = (timeoutMs: number): Promise<void> =>
    new Promise((resolve, reject) => {
      const timer = setTimeout(
        () => reject(new UsaiTestError(`websocket: nothing received within ${timeoutMs} ms`)),
        timeoutMs,
      );
      chunks.push(() => {
        clearTimeout(timer);
        resolve();
      });
    });
  // Head.
  let protocol: string | undefined;
  for (;;) {
    const end = buffer.indexOf("\r\n\r\n");
    if (end >= 0) {
      const head = buffer.subarray(0, end).toString();
      buffer = buffer.subarray(end + 4);
      const status = Number(head.split(" ")[1]);
      const headers: Record<string, string> = {};
      for (const line of head.split("\r\n").slice(1)) {
        const i = line.indexOf(":");
        if (i > 0) headers[line.slice(0, i).trim().toLowerCase()] = line.slice(i + 1).trim();
      }
      if (status !== 101) {
        const length = Number(headers["content-length"] ?? 0);
        while (buffer.length < length && !ended) await waitData(5000);
        const body = buffer.subarray(0, length).toString();
        socket.destroy();
        throw new TestSocketRefused(status, body);
      }
      const expected = createHash("sha1")
        .update(`${key}258EAFA5-E914-47DA-95CA-C5AB0DC85B11`)
        .digest("base64");
      if (headers["sec-websocket-accept"] !== expected) {
        socket.destroy();
        throw new UsaiTestError("websocket: bad Sec-WebSocket-Accept");
      }
      protocol = headers["sec-websocket-protocol"];
      break;
    }
    if (ended) throw new UsaiTestError("websocket: connection closed before the upgrade answered");
    await waitData(5000);
  }
  const frame = (opcode: number, payload: Buffer): Buffer => {
    const mask = randomBytes(4);
    const len = payload.length;
    const header =
      len < 126
        ? Buffer.from([0x80 | opcode, 0x80 | len])
        : len < 65536
          ? Buffer.concat([
              Buffer.from([0x80 | opcode, 0x80 | 126]),
              (() => {
                const b = Buffer.alloc(2);
                b.writeUInt16BE(len);
                return b;
              })(),
            ])
          : Buffer.concat([
              Buffer.from([0x80 | opcode, 0x80 | 127]),
              (() => {
                const b = Buffer.alloc(8);
                b.writeBigUInt64BE(BigInt(len));
                return b;
              })(),
            ]);
    const masked = Buffer.from(payload.map((byte, i) => byte ^ mask[i % 4]!));
    return Buffer.concat([header, mask, masked]);
  };
  const messages: Array<{ opcode: number; payload: Buffer }> = [];
  let closeFrame: { code: number; reason: string } | undefined;
  const drainFrames = () => {
    for (;;) {
      if (buffer.length < 2) return;
      const opcode = buffer[0]! & 0x0f;
      const masked = (buffer[1]! & 0x80) !== 0;
      let len = buffer[1]! & 0x7f;
      let at = 2;
      if (len === 126) {
        if (buffer.length < 4) return;
        len = buffer.readUInt16BE(2);
        at = 4;
      } else if (len === 127) {
        if (buffer.length < 10) return;
        len = Number(buffer.readBigUInt64BE(2));
        at = 10;
      }
      const maskLen = masked ? 4 : 0;
      if (buffer.length < at + maskLen + len) return;
      let payload = buffer.subarray(at + maskLen, at + maskLen + len);
      if (masked) {
        const mask = buffer.subarray(at, at + 4);
        payload = Buffer.from(payload.map((byte, i) => byte ^ mask[i % 4]!));
      }
      buffer = buffer.subarray(at + maskLen + len);
      if (opcode === 0x9) socket.write(frame(0xa, Buffer.from(payload)));
      else if (opcode === 0x8) {
        closeFrame = {
          code: payload.length >= 2 ? payload.readUInt16BE(0) : 1005,
          reason: payload.subarray(2).toString(),
        };
        ended = true;
      } else if (opcode === 0x1 || opcode === 0x2)
        messages.push({ opcode, payload: Buffer.from(payload) });
    }
  };
  const nextFrame = async (timeoutMs: number) => {
    const deadline = Date.now() + timeoutMs;
    for (;;) {
      drainFrames();
      if (messages.length) return messages.shift()!;
      if (ended) return null;
      const remaining = deadline - Date.now();
      if (remaining <= 0) throw new UsaiTestError(`websocket: no message within ${timeoutMs} ms`);
      await waitData(remaining);
    }
  };
  return {
    protocol,
    send: async (message) => {
      const text = typeof message === "string" ? message : JSON.stringify(message);
      socket.write(frame(0x1, Buffer.from(text)));
    },
    next: async <T>(timeoutMs = 5000): Promise<T | null> => {
      const f = await nextFrame(timeoutMs);
      if (!f) return null;
      const text = f.payload.toString();
      try {
        return JSON.parse(text) as T;
      } catch {
        return text as unknown as T;
      }
    },
    closed: async (timeoutMs = 5000) => {
      const deadline = Date.now() + timeoutMs;
      for (;;) {
        drainFrames();
        if (closeFrame) return closeFrame;
        if (ended) return { code: 1006, reason: "connection closed" };
        const remaining = deadline - Date.now();
        if (remaining <= 0)
          throw new UsaiTestError(`websocket: no close frame within ${timeoutMs} ms`);
        await waitData(remaining);
      }
    },
    close: async (code = 1000, reason = "") => {
      const payload = Buffer.alloc(2 + Buffer.byteLength(reason));
      payload.writeUInt16BE(code, 0);
      payload.write(reason, 2);
      socket.write(frame(0x8, payload));
      await new Promise<void>((resolve) => {
        const done = () => resolve();
        socket.once("close", done);
        setTimeout(() => {
          socket.destroy();
          resolve();
        }, 1000);
      });
    },
  };
}
