// Manifest extraction (ADR-0009): the build phase evaluates the application
// module in a capability-less world and calls `describe`. The shape mirrors
// `crates/usai-runtime/src/definition.rs` exactly.

import {
  type AppDeclaration,
  type Workload,
  flatten,
  parseDuration,
  workloadId,
} from "./declarations.ts";
import { jsonSchemaOf } from "./schema.ts";
import { hostFinal } from "./runtime/prepare.ts";

/** The manifest format this SDK writes; the runtime states which formats
 * it understands and refuses the others with a rebuild hint.
 *
 * @category Build */
export const MANIFEST_VERSION = 1 as const;
/** The host↔guest contract this SDK's in-world runtime speaks
 * (`docs/GUEST-ABI.md`). Stamped into `builtWith.abi`; a runtime with a
 * different bridge refuses the artifact at install instead of faulting
 * every world.
 *
 * @category Build */
export const GUEST_ABI = 1 as const;
/** The version of this SDK, stamped into manifests it describes (provenance
 * for compatibility diagnostics; not part of the application's identity).
 * Replaced at build time from package.json; the fallback is for source checkouts. */
declare const __USAI_SDK_VERSION__: string | undefined;
export const SDK_VERSION: string =
  typeof __USAI_SDK_VERSION__ === "string" ? __USAI_SDK_VERSION__ : "source";

export interface ManifestContracts {
  params?: Record<string, unknown>;
  query?: Record<string, unknown>;
  headers?: Record<string, unknown>;
  body?: Record<string, unknown>;
  input?: Record<string, unknown>;
  message?: Record<string, unknown>;
  response?: Record<string, Record<string, unknown>>;
  inWorldOnly?: string[];
  /** Input slots validated once, at the boundary: the schema's output is
   * provably its input, so the world does not parse them again. */
  boundaryFinal?: string[];
}

export interface ManifestWorkload {
  id: string;
  name: string;
  summary?: string;
  description?: string;
  module?: string;
  trigger: Record<string, unknown> & { kind: string };
  contracts: ManifestContracts;
  errors: Array<{ code: string; status: number }>;
  /** Documented response headers, by status (`"201"`, `"*"`). */
  responseHeaders?: Record<string, Record<string, string>>;
  /** The chosen OpenAPI operationId. */
  operationId?: string;
  auth?: string;
  resources: string[];
  dispatches: string[];
  publishes: string[];
  maxConcurrency?: number;
  timeoutMs?: number;
}

/** What `usai build` writes to `manifest.json`: the application as data —
 * every workload with its trigger, contracts (JSON Schema) and policies,
 * every resource with its secret-free configuration, the auth schemes,
 * the environment contract. Mirrors the runtime's `Manifest` exactly.
 *
 * @category Build */
export interface Manifest {
  manifestVersion: 1;
  name: string;
  description?: string;
  /** Response headers set on every application response. */
  headers?: Record<string, string>;
  modules: Array<{ name: string; migrations: string[]; seeders: string[] }>;
  workloads: ManifestWorkload[];
  resources: Array<{
    name: string;
    kind: string;
    module?: string;
    config: Record<string, unknown>;
    env: string[];
  }>;
  auth: Array<{
    name: string;
    scheme: string;
    header?: string;
    description?: string;
    credential?: { in: "header" | "cookie" | "query"; name: string };
  }>;
  env: Array<{ name: string; kind: string; required: boolean; values: string[] }>;
  codeSha256: string;
  builtWith?: { sdk?: string; runtime?: string; abi?: number };
}

function describeContracts(workload: Workload): ManifestContracts {
  const out: ManifestContracts = {};
  const inWorldOnly: string[] = [];
  const boundaryFinal: string[] = [];
  for (const slot of ["params", "query", "headers", "body", "input", "message"] as const) {
    const schema = workload.contracts[slot];
    if (!schema) continue;
    const json = jsonSchemaOf(schema, "input");
    if (json) {
      out[slot] = json;
      // The host validates request slots for HTTP and stream routes alike
      // (sockets carry no contracts for them); a final schema is parsed once.
      if ((workload.kind === "http" || workload.kind === "stream") && hostFinal(schema))
        boundaryFinal.push(slot);
    } else inWorldOnly.push(slot);
  }
  if (workload.contracts.response) {
    const response: Record<string, Record<string, unknown>> = {};
    for (const [status, schema] of Object.entries(workload.contracts.response)) {
      const json = jsonSchemaOf(schema, "output");
      if (json) response[status] = json;
      else inWorldOnly.push(`response.${status}`);
    }
    if (Object.keys(response).length > 0) out.response = response;
  }
  if (inWorldOnly.length > 0) out.inWorldOnly = inWorldOnly;
  if (boundaryFinal.length > 0) out.boundaryFinal = boundaryFinal;
  return out;
}

function trigger(workload: Workload): ManifestWorkload["trigger"] {
  switch (workload.kind) {
    case "http":
      return {
        kind: "http",
        method: workload.trigger["method"],
        path: workload.trigger["path"],
        raw: workload.trigger["raw"] === true,
        ...(workload.trigger["responses"] ? { responses: workload.trigger["responses"] } : {}),
      };
    case "cron": {
      const timeoutMs = parseDuration(workload.policies.timeout);
      return {
        kind: "cron",
        schedule: workload.trigger["schedule"],
        overlap: workload.trigger["overlap"] ?? "skip",
        ...(timeoutMs !== undefined ? { timeoutMs } : {}),
        ...(workload.trigger["exclusive"] ? { exclusive: true } : {}),
        ...(workload.trigger["database"] ? { database: workload.trigger["database"] } : {}),
      };
    }
    case "queue":
      return {
        kind: "queue",
        topic: workload.trigger["topic"],
        concurrency: workload.trigger["concurrency"] ?? 1,
        ...(workload.trigger["database"] !== undefined
          ? { database: workload.trigger["database"] }
          : {}),
        ...(workload.trigger["retry"] !== undefined ? { retry: workload.trigger["retry"] } : {}),
      };
    case "socket":
      return { kind: "socket", path: workload.trigger["path"] };
    case "stream":
      return {
        kind: "stream",
        method: workload.trigger["method"],
        path: workload.trigger["path"],
        ...(workload.trigger["contentType"]
          ? { content_type: workload.trigger["contentType"] }
          : {}),
      };
    case "service":
      return {
        kind: "service",
        ...(workload.trigger["restart"] !== undefined
          ? { restart: workload.trigger["restart"] }
          : {}),
      };
    default:
      return { kind: workload.kind };
  }
}

/** Turn an {@link AppDeclaration} into its {@link Manifest}. The build
 * phase calls it inside a capability-less world; call it yourself to
 * assert on an application's shape in a unit test. Throws on a duplicate
 * workload, a conflicting resource redeclaration, or a hole in a list.
 *
 * @category Build */
export function describe(app: AppDeclaration): Manifest {
  const { workloads, resources } = flatten(app);
  const authByName = new Map<string, Manifest["auth"][number]>();
  const manifestWorkloads: ManifestWorkload[] = workloads.map(({ workload, module }) => {
    if (workload.auth && !authByName.has(workload.auth.name)) {
      authByName.set(workload.auth.name, {
        name: workload.auth.name,
        scheme: workload.auth.scheme,
        ...(workload.auth.header ? { header: workload.auth.header } : {}),
        ...(workload.auth.description ? { description: workload.auth.description } : {}),
        ...(workload.auth.credential ? { credential: workload.auth.credential } : {}),
      });
    }
    const timeoutMs = parseDuration(workload.policies.timeout);
    const entry: ManifestWorkload = {
      id: workloadId(workload),
      name: workload.name,
      trigger: trigger(workload),
      contracts: describeContracts(workload),
      errors: workload.errors,
      resources: workload.resources.map((r) => r.name),
      dispatches: workload.dispatches.map((d) => workloadId(d)),
      publishes: [...workload.publishes],
    };
    if (module !== undefined) entry.module = module;
    if (workload.operationId) entry.operationId = workload.operationId;
    if (workload.responseHeaders) {
      const docs: Record<string, Record<string, string>> = {};
      for (const [status, headers] of Object.entries(workload.responseHeaders)) {
        const lower: Record<string, string> = {};
        for (const [name, text] of Object.entries(headers)) lower[name.toLowerCase()] = text;
        docs[String(status)] = lower;
      }
      entry.responseHeaders = docs;
    }
    if (workload.summary !== undefined) entry.summary = workload.summary;
    if (workload.description !== undefined) entry.description = workload.description;
    if (workload.auth) entry.auth = workload.auth.name;
    if (workload.policies.concurrency !== undefined)
      entry.maxConcurrency = workload.policies.concurrency;
    if (timeoutMs !== undefined) entry.timeoutMs = timeoutMs;
    return entry;
  });
  const env = app.env
    ? Object.entries(app.env.fields).map(([name, f]) => ({
        name,
        kind: f.kind,
        required: f.required,
        values: [...(f.values ?? [])],
      }))
    : [];
  return {
    manifestVersion: MANIFEST_VERSION,
    name: app.name,
    ...(app.description !== undefined ? { description: app.description } : {}),
    ...(app.headers !== undefined ? { headers: { ...app.headers } } : {}),
    modules: app.modules.map((m) => ({
      name: m.name,
      migrations: [...m.migrations],
      seeders: [...m.seeders],
    })),
    workloads: manifestWorkloads,
    resources: resources.map(({ resource, module }) => ({
      name: resource.name,
      kind: resource.kind,
      ...(module !== undefined ? { module } : {}),
      config: resource.config,
      env: [...resource.env],
    })),
    auth: [...authByName.values()],
    env,
    codeSha256: "",
    builtWith: { sdk: SDK_VERSION, abi: GUEST_ABI },
  };
}
