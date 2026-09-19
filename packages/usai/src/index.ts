/**
 * `@sakaladev/usai` — declare work and resources; the runtime gives each
 * its natural lifetime. Public surface for v0 (breaking changes allowed
 * before alpha). Start with {@link defineApp}, then {@link http},
 * {@link task}, {@link cron}, {@link queue}, {@link postgres}; every
 * handler receives a {@link BaseContext}. What *your* application
 * declares — every operation, what it leases and hands off, a request
 * panel — is the application reference at `/_usai/docs` on a running
 * `usai dev`.
 *
 * Vocabulary used throughout:
 * - **world** — the fresh, isolated execution of one unit of work (a
 *   request, a task, a cron tick, a queue message, a connection, a
 *   service). Nothing survives it except what it wrote to a resource.
 * - **commit** — the moment a world's work counts: its handler returned
 *   (for HTTP, the response head is sent; for a queue message, it is
 *   acknowledged). Hand-offs start after it; a throw means no commit.
 * - **lease** — a resource operation owned by the world for its duration
 *   (one connection per statement, one request per `fetch`); returned
 *   only after a terminal outcome.
 * - **hand-off** — `ctx.tasks.dispatch` or `ctx.queue.publish`: work that
 *   outlives the world, with a new owner, declared explicitly.
 * - **revision** — one immutable application definition installed in the
 *   runtime; a deploy installs a new one and **drains** the old (in-flight
 *   work finishes, persistent workloads are asked to stop).
 *
 * @module
 */

export { defineApp, defineModule } from "./declarations.ts";
export type {
  AppDeclaration,
  ModuleDeclaration,
  Workload,
  ResourceDeclaration,
  AuthDeclaration,
  DeclaredError,
  Method,
  DefineAppOptions,
  DefineModuleOptions,
  HttpOptions,
  HttpContracts,
  WorkloadPolicies,
  ResourcesOf,
} from "./declarations.ts";
export { http } from "./http.ts";
export type {
  HttpContext,
  RawContext,
  RawRequestBody,
  HttpResponse,
  RawResponse,
  RawOptions,
  HttpHandlerResult,
  Declare,
  RawHandler,
} from "./http.ts";
export type { AnySchema, Output } from "./schema.ts";
export { auth } from "./auth.ts";
export type { AuthRequest, BearerOptions, HeaderOptions, CustomOptions } from "./auth.ts";
export { task, cron, command, service, dispatches, publishes, seeder } from "./workloads.ts";
export type { SeederDeclaration, SeederContext } from "./workloads.ts";
export type {
  TaskContext,
  CronContext,
  CommandContext,
  ServiceContext,
  TaskOptions,
  CronOptions,
  CommandOptions,
  ServiceOptions,
} from "./workloads.ts";
export { socket } from "./connection.ts";
export type {
  StreamContext,
  StreamHandle,
  StreamOptions,
  SocketContext,
  SocketHandlers,
  SocketOptions,
} from "./connection.ts";
export { queue } from "./queue.ts";
export type { QueueContext, QueueHandle, RetryOptions, ConsumeOptions } from "./queue.ts";
export { cache, postgres, httpClient } from "./resources.ts";
export type {
  CacheLocalHandle,
  CacheLocalOptions,
  CacheLocalDeclaration,
  PostgresHandle,
  PostgresOptions,
  PostgresDeclaration,
  SqlExecutor,
  SqlParam,
  HttpClientHandle,
  HttpClientOptions,
  HttpClientDeclaration,
  FetchInit,
  FetchResponse,
} from "./resources.ts";
export { password } from "./password.ts";
export { errors, UsaiError, isUsaiError } from "./errors.ts";
export type { UsaiErrorShape } from "./errors.ts";
export { env, resolveEnv } from "./env.ts";
export type { EnvValues, EnvField, EnvDeclaration, EnvKind } from "./env.ts";
export type { StandardSchemaV1 } from "./schema.ts";
export type { BaseContext, UsaiAbortSignal, TaskHandle, ConsoleLike } from "./runtime/context.ts";
export { describe, MANIFEST_VERSION, GUEST_ABI } from "./manifest.ts";
export type { Manifest } from "./manifest.ts";

import { install } from "./runtime/sdk.ts";

// Registers `__usai_sdk` when evaluated inside a Usai world (or the build
// phase). Harmless elsewhere: it only sets a global.
install();
