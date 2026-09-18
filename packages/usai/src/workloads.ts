// Non-HTTP workload declarations. Declarations exist from D2 so the manifest
// and `inspect` know the whole application; runtime support arrives with
// D4 (task), D5 (cron, command), D9 (service).

import type { AnySchema, Output } from "./schema.ts";
import type { DeclaredError, ResourceDeclaration, Workload, WorkloadPolicies } from "./declarations.ts";
import type { BaseContext } from "./runtime/context.ts";

export interface TaskOptions<I extends AnySchema | undefined> extends WorkloadPolicies {
  input?: I;
  errors?: DeclaredError[];
  resources?: ResourceDeclaration[];
}

export interface TaskContext<I> extends BaseContext {
  readonly input: I;
}

export function task<I extends AnySchema | undefined = undefined>(
  name: string,
  options: TaskOptions<I>,
  handler: (ctx: TaskContext<I extends AnySchema ? Output<I> : unknown>) => unknown,
): Workload {
  const policies: WorkloadPolicies = {};
  if (options.timeout !== undefined) policies.timeout = options.timeout;
  if (options.concurrency !== undefined) policies.concurrency = options.concurrency;
  return {
    __usai: "workload",
    kind: "task",
    name,
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

export interface CronOptions extends WorkloadPolicies {
  schedule: string;
  overlap?: "allow" | "skip";
  resources?: ResourceDeclaration[];
}

export interface CronContext extends BaseContext {
  readonly scheduledAt: string;
}

export function cron(name: string, options: CronOptions, handler: (ctx: CronContext) => unknown): Workload {
  const policies: WorkloadPolicies = {};
  if (options.timeout !== undefined) policies.timeout = options.timeout;
  if (options.concurrency !== undefined) policies.concurrency = options.concurrency;
  return {
    __usai: "workload",
    kind: "cron",
    name,
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

export interface CommandContext extends BaseContext {
  readonly args: string[];
}

export function command(name: string, handler: (ctx: CommandContext) => unknown): Workload;
export function command(name: string, options: WorkloadPolicies & { resources?: ResourceDeclaration[] }, handler: (ctx: CommandContext) => unknown): Workload;
export function command(name: string, a: unknown, b?: unknown): Workload {
  const options = (typeof a === "function" ? {} : a) as WorkloadPolicies & { resources?: ResourceDeclaration[] };
  const handler = (typeof a === "function" ? a : b) as Workload["handler"];
  const policies: WorkloadPolicies = {};
  if (options.timeout !== undefined) policies.timeout = options.timeout;
  return {
    __usai: "workload",
    kind: "command",
    name,
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

export interface ServiceContext extends BaseContext {
  sleep(duration: string | number): Promise<void>;
}

export interface ServiceOptions {
  resources?: ResourceDeclaration[];
  /** What happens when the service ends. Default `never`: it stays ended
   * until the next revision. `on-failure` restarts after a throw; `always`
   * restarts whenever it ends. Backoff doubles per restart. */
  restart?: { mode: "never" | "on-failure" | "always"; backoffMs?: number; maxRestarts?: number };
}

export function service(name: string, handler: (ctx: ServiceContext) => unknown): Workload;
export function service(name: string, options: ServiceOptions, handler: (ctx: ServiceContext) => unknown): Workload;
export function service(name: string, a: unknown, b?: unknown): Workload {
  const options = (typeof a === "function" ? {} : a) as ServiceOptions;
  const handler = (typeof a === "function" ? a : b) as Workload["handler"];
  const restart = options.restart ? { mode: options.restart.mode, backoffMs: options.restart.backoffMs ?? 1000, maxRestarts: options.restart.maxRestarts ?? 10 } : undefined;
  return {
    __usai: "workload",
    kind: "service",
    name,
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

/** Records that `from` dispatches `to`, for `usai graph`. Returns `from`. */
export function dispatches(from: Workload, ...to: Workload[]): Workload {
  (from.dispatches as Workload[]).push(...to);
  return from;
}

/** Records that `from` publishes to queue topics (names, or the consuming
 * `queue.consume` workloads), for `usai graph` and the API docs. Returns `from`. */
export function publishes(from: Workload, ...topics: Array<string | Workload>): Workload {
  (from.publishes as string[]).push(...topics.map((t) => (typeof t === "string" ? t : t.name)));
  return from;
}

export interface SeederContext extends BaseContext {}

/** A seeder file's default export. Discovered by `usai db seed`, run as
 * finite work with access to the declared resources; never part of
 * startup. */
export interface SeederDeclaration {
  readonly __usai: "seeder";
  readonly resources: readonly ResourceDeclaration[];
  readonly run: (ctx: SeederContext) => unknown;
}

export function seeder(options: { resources?: ResourceDeclaration[] }, run: (ctx: SeederContext) => unknown): SeederDeclaration;
export function seeder(run: (ctx: SeederContext) => unknown): SeederDeclaration;
export function seeder(a: unknown, b?: unknown): SeederDeclaration {
  const options = (typeof a === "function" ? {} : a) as { resources?: ResourceDeclaration[] };
  const run = (typeof a === "function" ? a : b) as (ctx: SeederContext) => unknown;
  return { __usai: "seeder", resources: options.resources ?? [], run };
}
