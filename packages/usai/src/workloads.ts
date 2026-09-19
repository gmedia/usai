// Non-HTTP workload declarations. Declarations exist from D2 so the manifest
// and `inspect` know the whole application; runtime support arrives with
// D4 (task), D5 (cron, command), D9 (service).

import type { AnySchema, Output } from "./schema.ts";
import type {
  DeclaredError,
  ResourceDeclaration,
  ResourcesOf,
  Workload,
  WorkloadPolicies,
} from "./declarations.ts";
import type { BaseContext } from "./runtime/context.ts";

/** Options for {@link task}.
 *
 * @category Tasks, cron, commands, services
 */
export interface TaskOptions<
  I extends AnySchema | undefined,
  R extends ResourceDeclaration[] = ResourceDeclaration[],
> extends WorkloadPolicies {
  /** Schema for the input; validated before the task's world exists. */
  input?: I;
  /** A paragraph for the reference. */
  description?: string;
  /** Errors the handler throws, for the reference. */
  errors?: DeclaredError[];
  /** Resources this task leases; `ctx.resources` is typed from this list. */
  resources?: R;
}

/** The context of one task invocation: the validated `input` plus
 * {@link BaseContext}.
 *
 * @category Tasks, cron, commands, services
 */
export interface TaskContext<I, R = ResourceDeclaration[]> extends BaseContext {
  readonly input: I;
  readonly resources: ResourcesOf<R>;
}

/**
 * Declare a task: a named unit of finite work that other workloads invoke
 * or dispatch, and that `usai task run <name>` runs by hand.
 *
 * A task always runs in a **fresh world of its own**, never inside the
 * caller's. Who owns that world is the caller's choice at the call site:
 * `ctx.tasks.invoke(task, input)` is **owned** — the caller waits for the
 * result and the child is cancelled with the caller; `ctx.tasks.dispatch(task,
 * input)` is **ownership transfer** — the task runtime owns the child, the
 * caller's world may end, and the returned `{ id }` is the only handle.
 * There is no third option: a finite world that ends with live async work
 * is a runtime error. Dispatch is in-process and not durable across a
 * restart; for durable hand-off publish to a queue.
 *
 * @param name Unique within the application; the id is `task:<name>`.
 * Any text; a colon is fine (`invoices:remind`).
 * @param options Input schema, errors, resources, `timeout`, `concurrency`.
 * @param handler Runs in the task's world. Its return value is the
 * `invoke` result; after a `dispatch` nobody receives it — the outcome
 * shows only in the runtime's log and the task counters of
 * `/_usai/status`, so a dispatched task records what matters in a
 * resource.
 *
 * @example
 * ```ts
 * export const sendReceipt = task("send-receipt", { input: Receipt, resources: [db, mailer] }, async (ctx) => {
 *   const order = await ctx.resources.db.one("select … where id = $1", [ctx.input.orderId]);
 *   await ctx.resources.mailer.fetch("/send", { json: order });
 * });
 * // From an endpoint: hand it off, then answer.
 * export const pay = dispatches(
 *   http.post("/orders/:id/pay", { params: Id, resources: [db] }, async (ctx) => {
 *     await ctx.resources.db.execute("update orders set paid = true where id = $1", [ctx.params.id]);
 *     await ctx.tasks.dispatch(sendReceipt, { orderId: ctx.params.id });
 *     return http.accepted({ ok: true });
 *   }),
 *   sendReceipt,
 * );
 * ```
 *
 * @category Tasks, cron, commands, services
 */
export function task<
  I extends AnySchema | undefined = undefined,
  R extends ResourceDeclaration[] = ResourceDeclaration[],
>(
  name: string,
  options: TaskOptions<I, R>,
  handler: (ctx: TaskContext<I extends AnySchema ? Output<I> : unknown, R>) => unknown,
): Workload {
  const policies: WorkloadPolicies = {};
  if (options.timeout !== undefined) policies.timeout = options.timeout;
  if (options.concurrency !== undefined) policies.concurrency = options.concurrency;
  return {
    __usai: "workload",
    kind: "task",
    name,
    ...(options.description ? { description: options.description } : {}),
    trigger: {},
    contracts: options.input ? { input: options.input } : {},
    errors: options.errors ?? [],
    resources: options.resources ?? [],
    dispatches: [],
    publishes: [],
    policies,
    handler: handler as Workload["handler"],
  };
}

/** Options for {@link cron}.
 *
 * @category Tasks, cron, commands, services
 */
export interface CronOptions<R extends ResourceDeclaration[] = ResourceDeclaration[]>
  extends WorkloadPolicies {
  /** A paragraph for the reference. */
  description?: string;
  /** Cron expression, UTC: five fields (`minute hour day-of-month month
   * day-of-week`) or six with leading seconds. Validated at install. */
  schedule: string;
  /** What to do when a tick is due while the previous one still runs.
   * `skip` (default) drops the tick; `allow` starts another world. */
  overlap?: "allow" | "skip";
  /** Resources the tick leases; `ctx.resources` is typed from this list. */
  resources?: R;
}

/** The context of one cron tick: `scheduledAt` (ISO 8601, the tick's
 * nominal time) plus {@link BaseContext}.
 *
 * @category Tasks, cron, commands, services
 */
export interface CronContext<R = ResourceDeclaration[]> extends BaseContext {
  readonly scheduledAt: string;
  readonly resources: ResourcesOf<R>;
}

/**
 * Declare a scheduled job. Each due tick runs in a **fresh world**, finite,
 * bounded by `timeout`. The scheduler belongs to the revision: it starts
 * when the revision activates and stops when it drains, so two revisions
 * never tick the same job at once. A missed tick (the process was down)
 * is not replayed. `usai cron run <name>` runs one tick without the
 * clock, and `app.cron(name).run()` does the same in tests.
 *
 * @param name Unique within the application; the id is `cron:<name>`.
 * @param options `schedule` (required), `overlap`, resources, `timeout`.
 * @param handler Runs once per tick.
 *
 * @example
 * ```ts
 * export const markOverdue = publishes(
 *   cron("mark-overdue", { schedule: "15 0 * * *", resources: [db] }, async (ctx) => {
 *     const rows = await ctx.resources.db.query("update invoices … returning id");
 *     for (const row of rows) await ctx.queue.publish("webhook.deliver", { event: "invoice.overdue", id: row.id });
 *   }),
 *   "webhook.deliver",
 * );
 * ```
 *
 * @category Tasks, cron, commands, services
 */
export function cron<R extends ResourceDeclaration[] = ResourceDeclaration[]>(
  name: string,
  options: CronOptions<R>,
  handler: (ctx: CronContext<R>) => unknown,
): Workload {
  const policies: WorkloadPolicies = {};
  if (options.timeout !== undefined) policies.timeout = options.timeout;
  if (options.concurrency !== undefined) policies.concurrency = options.concurrency;
  return {
    __usai: "workload",
    kind: "cron",
    name,
    ...(options.description ? { description: options.description } : {}),
    trigger: { schedule: options.schedule, overlap: options.overlap ?? "skip" },
    contracts: {},
    errors: [],
    resources: options.resources ?? [],
    dispatches: [],
    publishes: [],
    policies,
    handler: handler as Workload["handler"],
  };
}

/** The context of one command run: the command-line `args` plus
 * {@link BaseContext}.
 *
 * @category Tasks, cron, commands, services
 */
export interface CommandContext<R = ResourceDeclaration[]> extends BaseContext {
  readonly args: string[];
  readonly resources: ResourcesOf<R>;
}

/** Options for {@link command}.
 *
 * @category Tasks, cron, commands, services */
export interface CommandOptions<R extends ResourceDeclaration[] = ResourceDeclaration[]>
  extends WorkloadPolicies {
  /** A paragraph for the reference. */
  description?: string;
  /** Resources the command leases; `ctx.resources` is typed from this list. */
  resources?: R;
}

/**
 * Declare a command: finite work run on demand from the command line
 * (`usai app <name> [args]`), in a fresh world with the declared resources.
 * The return value is printed as JSON; a thrown error exits non-zero.
 * Commands are for operators (a stats report, a one-off repair), not for
 * startup: nothing runs a command unless someone asks.
 *
 * @example
 * ```ts
 * export const stats = command("invoices:stats", { resources: [db] }, async (ctx) =>
 *   ctx.resources.db.one("select count(*)::int as invoices from invoices"),
 * );
 * ```
 *
 * @category Tasks, cron, commands, services
 */
export function command(name: string, handler: (ctx: CommandContext) => unknown): Workload;
export function command<R extends ResourceDeclaration[] = ResourceDeclaration[]>(
  name: string,
  options: CommandOptions<R>,
  handler: (ctx: CommandContext<R>) => unknown,
): Workload;
export function command(name: string, a: unknown, b?: unknown): Workload {
  const options = (typeof a === "function" ? {} : a) as CommandOptions;
  const handler = (typeof a === "function" ? a : b) as Workload["handler"];
  const policies: WorkloadPolicies = {};
  if (options.timeout !== undefined) policies.timeout = options.timeout;
  return {
    __usai: "workload",
    kind: "command",
    name,
    ...(options.description ? { description: options.description } : {}),
    trigger: {},
    contracts: {},
    errors: [],
    resources: options.resources ?? [],
    dispatches: [],
    publishes: [],
    policies,
    handler,
  };
}

/** The context of a service: {@link BaseContext} plus `sleep`. Watch
 * `ctx.signal` — it aborts when the revision drains, and `sleep` resolves
 * early then.
 *
 * @category Tasks, cron, commands, services
 */
export interface ServiceContext<R = ResourceDeclaration[]> extends BaseContext {
  sleep(duration: string | number): Promise<void>;
  readonly resources: ResourcesOf<R>;
}

/** Options for {@link service}.
 *
 * @category Tasks, cron, commands, services
 */
export interface ServiceOptions<R extends ResourceDeclaration[] = ResourceDeclaration[]> {
  /** A paragraph for the reference. */
  description?: string;
  /** Resources the service leases (per operation, like every world);
   * `ctx.resources` is typed from this list. */
  resources?: R;
  /** What happens when the service ends. Default `never`: it stays ended
   * until the next revision. `on-failure` restarts after a throw; `always`
   * restarts whenever it ends. Backoff doubles per restart. */
  restart?: { mode: "never" | "on-failure" | "always"; backoffMs?: number; maxRestarts?: number };
}

/**
 * Declare a service: the one **persistent** lifetime. One world starts
 * when the revision activates, runs the handler, and is asked to stop
 * (`ctx.signal` aborts) when the revision drains; a handler that ignores
 * the signal is cancelled at the drain bound. The handler returning or
 * throwing ends the service; `restart` decides what happens next. A
 * service is supervised per revision, so a replacement revision gets its
 * own instance and the old one stops with its revision.
 *
 * @example
 * ```ts
 * export const ticker = service("ticker", { resources: [cache], restart: { mode: "on-failure" } }, async (ctx) => {
 *   while (!ctx.signal.aborted) {
 *     await ctx.resources.cache.increment("ticks");
 *     await ctx.sleep("1s");
 *   }
 * });
 * ```
 *
 * @category Tasks, cron, commands, services
 */
export function service(name: string, handler: (ctx: ServiceContext) => unknown): Workload;
export function service<R extends ResourceDeclaration[] = ResourceDeclaration[]>(
  name: string,
  options: ServiceOptions<R>,
  handler: (ctx: ServiceContext<R>) => unknown,
): Workload;
export function service(name: string, a: unknown, b?: unknown): Workload {
  const options = (typeof a === "function" ? {} : a) as ServiceOptions;
  const handler = (typeof a === "function" ? a : b) as Workload["handler"];
  const restart = options.restart
    ? {
        mode: options.restart.mode,
        backoffMs: options.restart.backoffMs ?? 1000,
        maxRestarts: options.restart.maxRestarts ?? 10,
      }
    : undefined;
  return {
    __usai: "workload",
    kind: "service",
    name,
    ...(options.description ? { description: options.description } : {}),
    trigger: restart ? { restart } : {},
    contracts: {},
    errors: [],
    resources: options.resources ?? [],
    dispatches: [],
    publishes: [],
    policies: {},
    handler,
  };
}

/** Record that `from` invokes or dispatches the tasks `to`, so that `usai
 * graph`, `inspect` and the reference page show the edge. The annotation
 * is for the graph only: a dispatch to a task that is not listed here
 * still runs, and a dispatch to a task that does not exist is refused at
 * runtime (`unknown_task`) whether or not it is listed. Returns `from`, so
 * it wraps a declaration in place. See {@link QueueHandle.publish} and
 * {@link publishes} for the durable, cross-process equivalent.
 *
 * @category Tasks, cron, commands, services
 */
export function dispatches(from: Workload, ...to: Workload[]): Workload {
  (from.dispatches as Workload[]).push(...to);
  return from;
}

/** Record that `from` publishes to queue topics (names, or the consuming
 * `queue.consume` workloads) with {@link QueueHandle.publish}, for `usai
 * graph` and the reference page (the consumer's page lists its
 * publishers). Annotation only; the publish itself is `ctx.queue.publish`.
 * Returns `from`, so it wraps a declaration in place.
 *
 * @category Tasks, cron, commands, services
 */
export function publishes(from: Workload, ...topics: Array<string | Workload>): Workload {
  (from.publishes as string[]).push(...topics.map((t) => (typeof t === "string" ? t : t.name)));
  return from;
}

/** The context a seeder runs with: {@link BaseContext}.
 *
 * @category Tasks, cron, commands, services
 */
export interface SeederContext<R = ResourceDeclaration[]> extends BaseContext {
  readonly resources: ResourcesOf<R>;
}

/** A seeder file's default export. Discovered by `usai db seed`, run as
 * finite work with access to the declared resources; never part of
 * startup.
 *
 * @category Tasks, cron, commands, services
 */
export interface SeederDeclaration {
  readonly __usai: "seeder";
  readonly resources: readonly ResourceDeclaration[];
  readonly run: (ctx: SeederContext) => unknown;
}

/** Declare a seeder (the default export of a file matched by the
 * module's `seeders` globs). `usai db seed [name]` runs it as finite work
 * with the declared resources.
 *
 * @category Tasks, cron, commands, services
 */
export function seeder<R extends ResourceDeclaration[] = ResourceDeclaration[]>(
  options: { resources?: R },
  run: (ctx: SeederContext<R>) => unknown,
): SeederDeclaration;
export function seeder(run: (ctx: SeederContext) => unknown): SeederDeclaration;
export function seeder(a: unknown, b?: unknown): SeederDeclaration {
  const options = (typeof a === "function" ? {} : a) as { resources?: ResourceDeclaration[] };
  const run = (typeof a === "function" ? a : b) as (ctx: SeederContext) => unknown;
  return { __usai: "seeder", resources: options.resources ?? [], run };
}
