// Declaration objects. Everything a developer declares is a plain, inspectable
// object; the build phase reads these to produce the manifest (ADR-0009) and
// the in-world SDK reads the same objects to run handlers.

import type { AnySchema } from "./schema.ts";
import type { EnvDeclaration, EnvField } from "./env.ts";

export type Method = "GET" | "POST" | "PUT" | "PATCH" | "DELETE" | "HEAD" | "OPTIONS";

export interface DeclaredError {
  code: string;
  status: number;
}

export interface AuthDeclaration<Principal = unknown> {
  readonly __usai: "auth";
  readonly name: string;
  readonly scheme: "bearer" | "header" | "custom";
  readonly header?: string;
  readonly resolve: (ctx: unknown, credential: string | undefined) => Principal | Promise<Principal>;
}

export interface ResourceDeclaration {
  readonly __usai: "resource";
  readonly name: string;
  readonly kind: string;
  /** Normalized, secret-free configuration. */
  readonly config: Record<string, unknown>;
  /** Env variable names that participate in the resource identity. */
  readonly env: readonly string[];
  /** Methods the in-world proxy exposes. */
  readonly methods: readonly string[];
}

export interface WorkloadPolicies {
  /** Per-invocation deadline, e.g. "5s", "500ms", or milliseconds. */
  timeout?: string | number;
  /** Per-workload world budget (ADR-0012). */
  concurrency?: number;
}

export interface HttpContracts {
  params?: AnySchema;
  query?: AnySchema;
  headers?: AnySchema;
  body?: AnySchema;
  response?: AnySchema | Record<number, AnySchema>;
}

export interface HttpOptions extends HttpContracts, WorkloadPolicies {
  errors?: DeclaredError[];
  auth?: AuthDeclaration;
  resources?: ResourceDeclaration[];
}

export interface Workload {
  readonly __usai: "workload";
  readonly kind: "http" | "task" | "cron" | "command" | "service" | "queue" | "socket" | "stream";
  readonly name: string;
  readonly trigger: Record<string, unknown>;
  readonly contracts: {
    params?: AnySchema;
    query?: AnySchema;
    headers?: AnySchema;
    body?: AnySchema;
    input?: AnySchema;
    message?: AnySchema;
    response?: Record<number, AnySchema>;
  };
  readonly errors: DeclaredError[];
  readonly auth?: AuthDeclaration;
  readonly resources: ResourceDeclaration[];
  readonly dispatches: Workload[];
  readonly policies: WorkloadPolicies;
  readonly handler: (...args: never[]) => unknown;
}

export interface ModuleDeclaration {
  readonly __usai: "module";
  readonly name: string;
  readonly workloads: readonly Workload[];
  readonly resources: readonly ResourceDeclaration[];
  readonly migrations: readonly string[];
  readonly seeders: readonly string[];
}

export interface AppDeclaration {
  readonly __usai: "app";
  readonly name: string;
  readonly modules: readonly ModuleDeclaration[];
  readonly workloads: readonly Workload[];
  readonly resources: readonly ResourceDeclaration[];
  readonly env?: EnvDeclaration<Record<string, EnvField<unknown>>>;
}

export interface DefineModuleOptions {
  name: string;
  workloads?: Workload[];
  resources?: ResourceDeclaration[];
  migrations?: string | string[];
  seeders?: string | string[];
}

export function defineModule(options: DefineModuleOptions): ModuleDeclaration {
  return {
    __usai: "module",
    name: options.name,
    workloads: options.workloads ?? [],
    resources: options.resources ?? [],
    migrations: toList(options.migrations),
    seeders: toList(options.seeders),
  };
}

export interface DefineAppOptions {
  name?: string;
  modules?: ModuleDeclaration[];
  workloads?: Workload[];
  resources?: ResourceDeclaration[];
  env?: EnvDeclaration<Record<string, EnvField<unknown>>>;
}

export function defineApp(options: DefineAppOptions = {}): AppDeclaration {
  const app: AppDeclaration = {
    __usai: "app",
    name: options.name ?? "app",
    modules: options.modules ?? [],
    workloads: options.workloads ?? [],
    resources: options.resources ?? [],
    ...(options.env ? { env: options.env } : {}),
  };
  return app;
}

function toList(value: string | string[] | undefined): string[] {
  if (value === undefined) return [];
  return Array.isArray(value) ? value : [value];
}

/** Deterministic flattening used by both the manifest and the in-world
 * dispatcher, so workload ordinals agree (`docs/GUEST-ABI.md`). */
export function flatten(app: AppDeclaration): { workloads: Array<{ workload: Workload; module?: string }>; resources: Array<{ resource: ResourceDeclaration; module?: string }> } {
  const workloads: Array<{ workload: Workload; module?: string }> = [];
  const resources: Array<{ resource: ResourceDeclaration; module?: string }> = [];
  for (const module of app.modules) {
    for (const workload of module.workloads) workloads.push({ workload, module: module.name });
    for (const resource of module.resources) resources.push({ resource, module: module.name });
  }
  for (const workload of app.workloads) workloads.push({ workload });
  for (const resource of app.resources) resources.push({ resource });
  // Resources referenced by workloads but declared nowhere are implicitly
  // application-level, so a developer can declare once and reference.
  const seen = new Set(resources.map((r) => r.resource.name));
  for (const { workload } of workloads) {
    for (const resource of workload.resources) {
      if (!seen.has(resource.name)) {
        seen.add(resource.name);
        resources.push({ resource });
      }
    }
  }
  return { workloads, resources };
}

export function workloadId(workload: Workload): string {
  if (workload.kind === "http" || workload.kind === "stream") {
    return `${workload.kind}:${String(workload.trigger["method"])} ${String(workload.trigger["path"])}`;
  }
  return `${workload.kind}:${workload.name}`;
}

export function parseDuration(value: string | number | undefined): number | undefined {
  if (value === undefined) return undefined;
  if (typeof value === "number") return value;
  const match = /^(\d+(?:\.\d+)?)\s*(ms|s|m|h)?$/.exec(value.trim());
  if (!match) throw new Error(`invalid duration ${JSON.stringify(value)}`);
  const n = Number(match[1]);
  switch (match[2] ?? "ms") {
    case "ms": return n;
    case "s": return n * 1000;
    case "m": return n * 60_000;
    case "h": return n * 3_600_000;
    default: return n;
  }
}
