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
export interface AuthDeclaration<
  Principal = unknown,
  R extends readonly ResourceDeclaration[] = readonly ResourceDeclaration[],
> {
  /** @internal */
  readonly __usai: "auth";
  readonly name: string;
  /** For the OpenAPI security scheme: where the credential comes from. */
  readonly description?: string;
  readonly scheme: "bearer" | "header" | "cookie" | "custom";
  readonly header?: string;
  /** For a custom scheme: where the credential travels, so the OpenAPI
   * document and the reference describe it truthfully (a cookie, a query
   * parameter, a header). Without it the document says only that the
   * resolver reads the request. */
  readonly credential?: CredentialLocation;
  /** The resources the resolver leases. A workload that uses the scheme
   * gets them in addition to its own (`describe` merges the two lists), so
   * a route never has to repeat the session table for the resolver's sake;
   * they are typed on the workload's `ctx.resources` too. */
  readonly resources: R;
  readonly resolve: (
    ctx: unknown,
    credential: string | undefined,
  ) => Principal | Promise<Principal>;
}

/** Where a custom scheme's credential travels: `{ in: "cookie", name: "sid" }`,
 * `{ in: "header", name: "x-session" }`, `{ in: "query", name: "token" }`.
 *
 * @category Authentication
 */
export interface CredentialLocation {
  readonly in: "header" | "cookie" | "query";
  readonly name: string;
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
   * a failed attempt, and a **stream** ends — the client keeps the 200 it
   * already has and the body stops there. Undeclared: the runtime default
   * (30 s) for requests, tasks, cron ticks, queue messages, commands,
   * migrations and seeders; **none** for a stream, a socket or a service,
   * which would be useless with one. A deadline you declare is honoured
   * whatever the kind — it is the only way to bound an export. */
  timeout?: string | number;
  /** How many worlds of this workload may run at once. Past the bound the
   * next one is refused, never queued: an HTTP request gets 503
   * `capacity_exhausted`, `ctx.tasks.invoke`/`dispatch` reject with the
   * same code in the caller's world, a queue consumer simply claims fewer
   * messages (ADR-0012). */
  concurrency?: number;
  /** Request body bound for this route, in bytes. A **cap**, never a raise:
   * the effective bound is the smaller of this and the process's
   * `USAI_MAX_BODY_BYTES` (1 MiB by default), so the operator keeps the
   * ceiling and each route decides how much of it to accept. Declare a small
   * one on ordinary routes and raise the process bound for the one that
   * takes uploads, instead of opening every route to the largest body any
   * of them needs. Above the bound the request is `413 payload_too_large`,
   * decided before a world exists. */
  maxBodyBytes?: number;
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

/** Response headers an endpoint sets, documented per status for the
 * reference and the OpenAPI document (`responses[status].headers`): the
 * key is the status (`201`, `200`, or `"*"` for every status), the value
 * maps a header name to one line about it. Descriptive — the runtime does
 * not validate them; a generated client learns they exist.
 *
 * @example
 * ```ts
 * responseHeaders: { 201: { location: "URL of the new user" }, "*": { etag: "Version of the resource" } }
 * ```
 *
 * @category Application
 */
export type ResponseHeaderDocs = Record<number | "*", Record<string, string>>;

/** Options of `http.get`/`post`/…: contracts, policies, errors, auth, resources.
 *
 * @category Application
 */
/** Rejects option keys the shape does not declare.
 *
 * A declaration whose options parameter is the inferred type itself (so that
 * `ctx` and the response type can be read off it) loses TypeScript's
 * excess-property check: the unknown key simply widens the inferred type. A
 * typo then compiles, builds and serves — `Auth:` for `auth:` is a route
 * without its authentication. Mapping every key the shape does not declare to
 * `never` puts the check back without giving up the inference.
 *
 * @category Advanced
 */
export type NoExtraKeys<O, Shape> = O & Record<Exclude<keyof O, keyof Shape>, never>;

export interface HttpOptions extends HttpContracts, WorkloadPolicies {
  /** One line for the reference and the OpenAPI `summary`. Without it the
   * operation is shown by method and path. */
  summary?: string;
  /** A paragraph for the reference and the OpenAPI `description`. */
  description?: string;
  /** Errors the handler throws, for the reference and the OpenAPI document. */
  errors?: DeclaredError[];
  /** Response headers the handler sets (`set-cookie`, `location`, `etag`),
   * documented per status. See {@link ResponseHeaderDocs}. */
  responseHeaders?: ResponseHeaderDocs;
  /** The OpenAPI `operationId` — what a generated client names the method
   * (`listProducts`). Derived from method and path when absent
   * (`getProducts`). Unique per application. */
  operationId?: string;
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
/** A {@link Workload} that remembers what its handler returns, so
 * `ctx.tasks.invoke(thatTask)` is typed instead of `unknown`. The parameter
 * is a phantom: nothing carries it at runtime, and a `TypedWorkload` is a
 * `Workload` everywhere one is expected.
 *
 * @category Declarations
 */
export interface TypedWorkload<Out = unknown> extends Workload {
  /** @internal phantom — never present at runtime */
  readonly __output?: Out;
}

/** What `ctx.tasks.invoke` resolves to for a given task declaration.
 *
 * @category Declarations
 */
export type TaskOutput<W> = W extends TypedWorkload<infer Out> ? Out : unknown;

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
    /** A stream's events by name (`http.stream({ events })`). */
    events?: Record<string, AnySchema>;
  };
  readonly errors: DeclaredError[];
  /** Documented response headers per status (HTTP, raw and stream workloads). */
  readonly responseHeaders?: ResponseHeaderDocs;
  /** The chosen OpenAPI `operationId`, when the declaration set one. */
  readonly operationId?: string;
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
  /** The directory the declaration was written in (stamped by the build). */
  readonly sourceDir?: string;
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
  /** Response headers set on every application response (`defineApp({ headers })`). */
  readonly headers?: Readonly<Record<string, string>>;
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
  /** Glob(s) for this module's SQL migrations. **Write them relative to the
   * module's own file** (`./migrations/*.sql` in `src/billing/module.ts`);
   * a glob is also tried as written from the project root, so
   * `./src/billing/migrations/*.sql` — the only form earlier versions
   * accepted — keeps working. */
  migrations?: string | string[];
  /** Glob(s) for this module's seeder files, relative to the module's own
   * file or to the project root (the same rule as `migrations`). */
  seeders?: string | string[];
  /** Filled in by the build, not by you: the directory the `defineModule`
   * call was written in, relative to the project root. It is what lets a
   * module's globs be written relative to the module itself — the runtime
   * tries a glob as written first and falls back to this directory. */
  sourceDir?: string;
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
    ...(options.sourceDir === undefined ? {} : { sourceDir: options.sourceDir }),
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
  /** Response headers set on every response of the application's routes
   * (not on `/_usai/*`): the security headers a proxy would otherwise add
   * (`strict-transport-security`, `x-content-type-options`,
   * `content-security-policy` …). A handler's own header of the same name
   * wins. Static values only — anything computed belongs in the handler. */
  headers?: Record<string, string>;
  /** The modules this application is composed of (`defineModule`). A module
   * brings its own workloads, resources, migrations and seeders, so a larger
   * application lists modules here and workloads nowhere. */
  modules?: ModuleDeclaration[];
  /** The workloads that do not belong to a module — routes, tasks, cron
   * entries, consumers, services, commands. **A workload exists because this
   * list (or a module's) reaches it through an `import`: the runtime never
   * scans your files**, so a route file nobody imports is not served. */
  workloads?: Workload[];
  /** Resources the application opens that no workload declares — rare: a
   * resource named in a workload's `resources: [...]` is already part of the
   * application. Everything here is opened at activation. */
  resources?: ResourceDeclaration[];
  /** The environment the whole application requires (`env({ … })`): every
   * variable, its type and whether it is required. A missing required
   * variable fails activation with all of them named at once, never at the
   * first request. */
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
    ...(options.headers && Object.keys(options.headers).length
      ? {
          headers: Object.fromEntries(
            Object.entries(options.headers).map(([k, v]) => [k.toLowerCase(), v]),
          ),
        }
      : {}),
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

/** The resources a workload's auth scheme contributes to its `ctx.resources`
 * type: the scheme's declared list when it has one, nothing when the scheme
 * left it untyped.
 *
 * @category Application */
export type AuthResourcesOf<A> =
  A extends AuthDeclaration<unknown, infer R>
    ? [readonly ResourceDeclaration[]] extends [R]
      ? Record<never, never>
      : ResourcesOf<R>
    : Record<never, never>;

/** A workload's resources plus the ones its auth scheme leases, each
 * declaration once (by name).
 *
 * @internal */
export function withAuthResources(
  own: readonly ResourceDeclaration[] | undefined,
  auth: AuthDeclaration<unknown, readonly ResourceDeclaration[]> | undefined,
): ResourceDeclaration[] {
  const out: ResourceDeclaration[] = [...(own ?? [])];
  for (const r of auth?.resources ?? []) if (!out.some((o) => o.name === r.name)) out.push(r);
  return out;
}
