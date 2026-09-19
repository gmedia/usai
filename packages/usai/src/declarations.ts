// Declaration objects. Everything a developer declares is a plain, inspectable
// object; the build phase reads these to produce the manifest (ADR-0009) and
// the in-world SDK reads the same objects to run handlers.

import type { AnySchema } from "./schema.ts";
import type { EnvDeclaration, EnvField } from "./env.ts";

/** HTTP methods an endpoint can declare.
 *
 * @category Application
 */
export type Method = "GET" | "POST" | "PUT" | "PATCH" | "DELETE" | "HEAD" | "OPTIONS";

/** An error a workload declares it may answer with (`errors: [...]`), so
 * the reference and the OpenAPI document list it.
 *
 * @category Application
 */
export interface DeclaredError {
  code: string;
  status: number;
}

/** What `auth.bearer`/`auth.header`/`auth.custom` return: a named
 * boundary reused by reference. `Principal` is the type of `ctx.auth`.
 *
 * @category Application
 */
export interface AuthDeclaration<Principal = unknown> {
  /** @internal */
  readonly __usai: "auth";
  readonly name: string;
  /** For the OpenAPI security scheme: where the credential comes from. */
  readonly description?: string;
  readonly scheme: "bearer" | "header" | "custom";
  readonly header?: string;
  readonly resolve: (
    ctx: unknown,
    credential: string | undefined,
  ) => Principal | Promise<Principal>;
}

/** What `postgres(...)`, `cache.local(...)` and `httpClient(...)` return.
 * A plain object: the build reads it into the manifest, the runtime owns
 * the resource it names, and a workload lists it under `resources`.
 *
 * @category Application
 */
export interface ResourceDeclaration<Name extends string = string, Handle = unknown> {
  /** @internal */
  readonly __usai: "resource";
  readonly name: Name;
  readonly kind: string;
  /** @internal Phantom: the in-world handle type, so `ctx.resources` can be typed from `resources: [...]`. */
  readonly __handle?: Handle;
  /** Normalized, secret-free configuration. */
  readonly config: Record<string, unknown>;
  /** Env variable names that participate in the resource identity. */
  readonly env: readonly string[];
  /** Methods the in-world proxy exposes. */
  readonly methods: readonly string[];
}

/** `ctx.resources` for a workload that declared `resources: R`: one
 * property per declaration, named by the resource, typed as its in-world
 * handle ({@link PostgresHandle}, {@link CacheLocalHandle},
 * {@link HttpClientHandle}). Without a declaration list it is
 * `Record<string, unknown>`.
 *
 * @category Application */
export type ResourcesOf<R> = R extends readonly ResourceDeclaration[]
  ? {
      readonly [D in R[number] as D["name"]]: D extends ResourceDeclaration<string, infer H>
        ? H
        : unknown;
    }
  : Record<string, unknown>;

/** Bounds every workload can declare.
 *
 * @category Application
 */
export interface WorkloadPolicies {
  /** Per-invocation deadline (`"5s"`, `"500ms"`, or milliseconds). The
   * world is cancelled when it passes: an HTTP caller gets 504, an
   * `invoke` rejects with `deadline_exceeded`, a queue message counts as
   * a failed attempt. Undeclared: the runtime default (30 s) for
   * requests, none for the other kinds. */
  timeout?: string | number;
  /** How many worlds of this workload may run at once. Past the bound the
   * next one is refused, never queued: an HTTP request gets 503
   * `capacity_exhausted`, `ctx.tasks.invoke`/`dispatch` reject with the
   * same code in the caller's world, a queue consumer simply claims fewer
   * messages (ADR-0012). */
  concurrency?: number;
}

/** The schema slots of an HTTP endpoint. Any Standard Schema
 * (`zod`, `valibot`, `arktype`, …) works; slots whose schema can describe
 * itself as JSON Schema are validated before a world exists, the others
 * inside it.
 *
 * @category Application
 */
export interface HttpContracts {
  /** Path parameters (`/users/:id` → `{ id }`); strings before coercion. */
  params?: AnySchema;
  /** Query string; a repeated key arrives as an array. */
  query?: AnySchema;
  /** Request headers, lower-cased names. */
  headers?: AnySchema;
  /** JSON request body. */
  body?: AnySchema;
  /** One schema (status 200) or a map of status to schema. A plain return
   * value is encoded with the lowest declared 2xx status and checked
   * against its schema (`response_contract_violation`, 500, otherwise);
   * `http.response(status, body)` picks another declared status. */
  response?: AnySchema | Record<number, AnySchema>;
}

/** Options of `http.get`/`post`/…: contracts, policies, errors, auth, resources.
 *
 * @category Application
 */
export interface HttpOptions extends HttpContracts, WorkloadPolicies {
  /** One line for the reference and the OpenAPI `summary`. Without it the
   * operation is shown by method and path. */
  summary?: string;
  /** A paragraph for the reference and the OpenAPI `description`. */
  description?: string;
  /** Errors the handler throws, for the reference and the OpenAPI document. */
  errors?: DeclaredError[];
  /** The authentication boundary; its principal is `ctx.auth`. */
  auth?: AuthDeclaration;
  /** Resources this endpoint leases; only these are on `ctx.resources`. */
  resources?: ResourceDeclaration[];
}

/** A declared unit of work, whatever its kind — what every `http.*`,
 * `task`, `cron`, `command`, `service`, `queue.consume`, `socket` and
 * `http.stream` call returns and what `defineApp`/`defineModule` list.
 * Plain data: the build phase reads it into the manifest, the runtime
 * routes to it, `inspect`/`graph`/the reference page render it.
 *
 * @category Application
 */
export interface Workload {
  /** @internal */
  readonly __usai: "workload";
  readonly kind: "http" | "task" | "cron" | "command" | "service" | "queue" | "socket" | "stream";
  readonly name: string;
  /** One line about the workload, for the reference and the OpenAPI `summary`. */
  readonly summary?: string;
  /** A paragraph about the workload, for the reference and the OpenAPI `description`. */
  readonly description?: string;
  /** Kind-specific facts (method and path, schedule, topic, …), as the manifest carries them. */
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
  /** Queue topics this workload publishes to (`publishes(...)`). */
  readonly publishes: string[];
  readonly policies: WorkloadPolicies;
  /** @internal The handler, erased; the in-world dispatcher calls it with the kind's context. */
  readonly handler: (...args: never[]) => unknown;
}

/** What {@link defineModule} returns.
 *
 * @category Application
 */
export interface ModuleDeclaration {
  /** @internal */
  readonly __usai: "module";
  readonly name: string;
  readonly workloads: readonly Workload[];
  readonly resources: readonly ResourceDeclaration[];
  readonly migrations: readonly string[];
  readonly seeders: readonly string[];
}

/** What {@link defineApp} returns: the application's default export.
 *
 * @category Application
 */
export interface AppDeclaration {
  /** @internal */
  readonly __usai: "app";
  readonly name: string;
  readonly description?: string;
  readonly modules: readonly ModuleDeclaration[];
  readonly workloads: readonly Workload[];
  readonly resources: readonly ResourceDeclaration[];
  readonly env?: EnvDeclaration<Record<string, EnvField<unknown>>>;
}

/** Options for {@link defineModule}.
 *
 * @category Application
 */
export interface DefineModuleOptions {
  /** Module name: groups operations in the reference and OpenAPI tags. */
  name: string;
  workloads?: Workload[];
  /** Resources this module declares; the same resource may be declared by
   * several modules with identical configuration. */
  resources?: ResourceDeclaration[];
  /** Glob(s) for this module's SQL migrations, relative to the project root
   * (e.g. `./src/billing/migrations/*.sql`). The bundle carries no source
   * locations, so module-relative paths are not supported in v0. */
  migrations?: string | string[];
  /** Glob(s) for this module's seeder files, relative to the project root. */
  seeders?: string | string[];
}

/**
 * Group workloads, resources, migrations and seeders under a name. A
 * module is organisation, not isolation: its workloads run like any other,
 * and a resource it declares is shared with every module that declares the
 * same one. Modules are the unit that owns SQL migrations.
 *
 * @example
 * ```ts
 * export const invoices = defineModule({
 *   name: "invoices",
 *   workloads: [list, get, create, issue, pay, markOverdue],
 *   resources: [db],
 *   migrations: "./src/invoices/migrations/*.sql",
 * });
 * ```
 *
 * @category Application
 */
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

/** Options for {@link defineApp}.
 *
 * @category Application */
export interface DefineAppOptions {
  /** Application name: the OpenAPI title and the reference page's heading. */
  name?: string;
  /** One paragraph about the application, shown on the reference page's
   * overview and as `info.description` of the generated OpenAPI document.
   * Plain text (no markup). Not part of the application identity. */
  description?: string;
  modules?: ModuleDeclaration[];
  workloads?: Workload[];
  resources?: ResourceDeclaration[];
  env?: EnvDeclaration<Record<string, EnvField<unknown>>>;
}

/**
 * The application root: the default export of the entry file. Everything
 * the runtime will ever run is reachable from here — modules, top-level
 * workloads, resources and the environment contract — which is why
 * `usai inspect`, `usai graph`, the OpenAPI document and the reference
 * page all read this one object. Declaring a workload twice, or leaving
 * a hole in a list (a `const` used before it ran), is a build error that
 * names the slot.
 *
 * @example
 * ```ts
 * export default defineApp({
 *   name: "invoicing",
 *   description: "Multi-tenant invoicing with webhook delivery.",
 *   modules: [authModule, invoices, webhooks],
 *   env: env({ DATABASE_URL: env.url(), SESSION_TTL_HOURS: env.optional(env.int()) }),
 * });
 * ```
 *
 * @category Application
 */
export function defineApp(options: DefineAppOptions = {}): AppDeclaration {
  const app: AppDeclaration = {
    __usai: "app",
    name: options.name ?? "app",
    ...(options.description ? { description: options.description } : {}),
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
export function flatten(app: AppDeclaration): {
  workloads: Array<{ workload: Workload; module?: string }>;
  resources: Array<{ resource: ResourceDeclaration; module?: string }>;
} {
  const workloads: Array<{ workload: Workload; module?: string }> = [];
  const resources: Array<{ resource: ResourceDeclaration; module?: string }> = [];
  // One logical resource may be declared by several modules (a shared
  // database). Same name + same kind + same config is one resource, owned by
  // the first declarer; a conflicting redeclaration is a build error.
  const add = (resource: ResourceDeclaration, module: string | undefined, where: string) => {
    const existing = resources.find((r) => r.resource.name === resource.name);
    if (!existing) {
      resources.push(module === undefined ? { resource } : { resource, module });
      return;
    }
    const same =
      existing.resource.kind === resource.kind &&
      JSON.stringify(existing.resource.config) === JSON.stringify(resource.config) &&
      JSON.stringify(existing.resource.env) === JSON.stringify(resource.env);
    if (!same) {
      throw new Error(
        `resource "${resource.name}" is declared twice with different configuration (${existing.module ?? "app"} and ${where})`,
      );
    }
  };
  // A hole in a declaration list is almost always a value used before its
  // `const` ran (declared below the module that lists it, or a circular
  // import). Name the slot instead of failing later on `undefined`.
  const check = (list: readonly unknown[], what: string, where: string) => {
    list.forEach((entry, i) => {
      const e = entry as { __usai?: string } | undefined;
      if (!e || typeof e !== "object" || !e.__usai) {
        throw new Error(
          `${where}: ${what}[${i}] is ${e === undefined ? "undefined" : typeof e} — is it declared after the \`defineModule\`/\`defineApp\` that lists it, or imported from a module that imports this one?`,
        );
      }
    });
  };
  for (const module of app.modules ?? []) {
    if (!module || typeof module !== "object")
      throw new Error(
        `defineApp: modules contains ${module === undefined ? "undefined" : typeof module} — declared after use or a circular import?`,
      );
    check(module.workloads, "workloads", `module "${module.name}"`);
    check(module.resources, "resources", `module "${module.name}"`);
    for (const workload of module.workloads) workloads.push({ workload, module: module.name });
    for (const resource of module.resources) add(resource, module.name, `module ${module.name}`);
  }
  check(app.workloads, "workloads", `app "${app.name}"`);
  check(app.resources, "resources", `app "${app.name}"`);
  for (const workload of app.workloads) workloads.push({ workload });
  for (const resource of app.resources) add(resource, undefined, "app");
  // Resources referenced by workloads but declared nowhere are implicitly
  // application-level, so a developer can declare once and reference.
  for (const { workload, module } of workloads) {
    for (const resource of workload.resources)
      add(
        resource,
        undefined,
        module ? `workload ${workload.name} in ${module}` : `workload ${workload.name}`,
      );
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
    case "ms":
      return n;
    case "s":
      return n * 1000;
    case "m":
      return n * 60_000;
    case "h":
      return n * 3_600_000;
    default:
      return n;
  }
}
