// Usai SDK — declare work and resources; the runtime gives each its natural
// lifetime. Public surface for v0 (breaking changes allowed before alpha).

export { defineApp, defineModule } from "./declarations.ts";
export type { AppDeclaration, ModuleDeclaration, Workload, ResourceDeclaration, AuthDeclaration, DeclaredError, Method } from "./declarations.ts";
export { http } from "./http.ts";
export type { HttpContext, RawContext, HttpResponse, RawResponse } from "./http.ts";
export { auth } from "./auth.ts";
export { task, cron, command, service, dispatches, seeder } from "./workloads.ts";
export type { SeederDeclaration, SeederContext } from "./workloads.ts";
export type { TaskContext, CronContext, CommandContext, ServiceContext } from "./workloads.ts";
export { socket } from "./connection.ts";
export type { StreamContext, StreamHandle, SocketContext, SocketHandlers } from "./connection.ts";
export { queue } from "./queue.ts";
export type { QueueContext, QueueHandle, RetryOptions } from "./queue.ts";
export { cache, postgres, httpClient } from "./resources.ts";
export type { CacheLocalHandle, PostgresHandle, SqlExecutor, SqlParam, HttpClientHandle, HttpClientOptions, FetchInit, FetchResponse } from "./resources.ts";
export { password } from "./password.ts";
export { errors, UsaiError, isUsaiError } from "./errors.ts";
export { env, resolveEnv } from "./env.ts";
export type { EnvValues } from "./env.ts";
export type { StandardSchemaV1 } from "./schema.ts";
export type { BaseContext, UsaiAbortSignal } from "./runtime/context.ts";
export { describe, MANIFEST_VERSION } from "./manifest.ts";
export type { Manifest } from "./manifest.ts";

import { install } from "./runtime/sdk.ts";

// Registers `__usai_sdk` when evaluated inside a Usai world (or the build
// phase). Harmless elsewhere: it only sets a global.
install();
