// Manifest extraction (ADR-0009): the build phase evaluates the application
// module in a capability-less world and calls `describe`. The shape mirrors
// `crates/usai-runtime/src/definition.rs` exactly.

import { type AppDeclaration, type Workload, flatten, parseDuration, workloadId } from "./declarations.ts";
import { jsonSchemaOf } from "./schema.ts";

export const MANIFEST_VERSION = 1 as const;

export interface ManifestContracts {
  params?: Record<string, unknown>;
  query?: Record<string, unknown>;
  headers?: Record<string, unknown>;
  body?: Record<string, unknown>;
  input?: Record<string, unknown>;
  message?: Record<string, unknown>;
  response?: Record<string, Record<string, unknown>>;
  inWorldOnly?: string[];
}

export interface ManifestWorkload {
  id: string;
  name: string;
  module?: string;
  trigger: Record<string, unknown> & { kind: string };
  contracts: ManifestContracts;
  errors: Array<{ code: string; status: number }>;
  auth?: string;
  resources: string[];
  dispatches: string[];
  maxConcurrency?: number;
  timeoutMs?: number;
}

export interface Manifest {
  manifestVersion: 1;
  name: string;
  modules: Array<{ name: string; migrations: string[]; seeders: string[] }>;
  workloads: ManifestWorkload[];
  resources: Array<{ name: string; kind: string; module?: string; config: Record<string, unknown>; env: string[] }>;
  auth: Array<{ name: string; scheme: string; header?: string }>;
  env: Array<{ name: string; kind: string; required: boolean; values: string[] }>;
  codeSha256: string;
}

function describeContracts(workload: Workload): ManifestContracts {
  const out: ManifestContracts = {};
  const inWorldOnly: string[] = [];
  for (const slot of ["params", "query", "headers", "body", "input", "message"] as const) {
    const schema = workload.contracts[slot];
    if (!schema) continue;
    const json = jsonSchemaOf(schema, "input");
    if (json) out[slot] = json;
    else inWorldOnly.push(slot);
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
  return out;
}

function trigger(workload: Workload): ManifestWorkload["trigger"] {
  switch (workload.kind) {
    case "http":
      return { kind: "http", method: workload.trigger["method"], path: workload.trigger["path"], raw: workload.trigger["raw"] === true };
    case "cron": {
      const timeoutMs = parseDuration(workload.policies.timeout);
      return { kind: "cron", schedule: workload.trigger["schedule"], overlap: workload.trigger["overlap"] ?? "skip", ...(timeoutMs !== undefined ? { timeoutMs } : {}) };
    }
    case "queue":
      return {
        kind: "queue",
        topic: workload.trigger["topic"],
        concurrency: workload.trigger["concurrency"] ?? 1,
        ...(workload.trigger["database"] !== undefined ? { database: workload.trigger["database"] } : {}),
        ...(workload.trigger["retry"] !== undefined ? { retry: workload.trigger["retry"] } : {}),
      };
    case "socket":
      return { kind: "socket", path: workload.trigger["path"] };
    case "stream":
      return { kind: "stream", method: workload.trigger["method"], path: workload.trigger["path"] };
    default:
      return { kind: workload.kind };
  }
}

export function describe(app: AppDeclaration): Manifest {
  const { workloads, resources } = flatten(app);
  const authByName = new Map<string, { name: string; scheme: string; header?: string }>();
  const manifestWorkloads: ManifestWorkload[] = workloads.map(({ workload, module }) => {
    if (workload.auth && !authByName.has(workload.auth.name)) {
      authByName.set(workload.auth.name, { name: workload.auth.name, scheme: workload.auth.scheme, ...(workload.auth.header ? { header: workload.auth.header } : {}) });
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
    };
    if (module !== undefined) entry.module = module;
    if (workload.auth) entry.auth = workload.auth.name;
    if (workload.policies.concurrency !== undefined) entry.maxConcurrency = workload.policies.concurrency;
    if (timeoutMs !== undefined) entry.timeoutMs = timeoutMs;
    return entry;
  });
  const env = app.env
    ? Object.entries(app.env.fields).map(([name, f]) => ({ name, kind: f.kind, required: f.required, values: [...(f.values ?? [])] }))
    : [];
  return {
    manifestVersion: MANIFEST_VERSION,
    name: app.name,
    modules: app.modules.map((m) => ({ name: m.name, migrations: [...m.migrations], seeders: [...m.seeders] })),
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
  };
}
