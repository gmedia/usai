// Queue / message workloads (`GOAL.md` §19). Persistent consumer
// infrastructure, a fresh world per message, explicit retry (ADR-0014).

import type { AnySchema, Output } from "./schema.ts";
import type { DeclaredError, ResourceDeclaration, Workload, WorkloadPolicies } from "./declarations.ts";
import type { PostgresDeclaration } from "./resources.ts";
import type { BaseContext } from "./runtime/context.ts";

export interface RetryOptions {
  /** Total attempts including the first. Default 1: failure is terminal. */
  maxAttempts: number;
  backoff?: "fixed" | "exponential";
  /** Base delay in ms (default 1000). */
  baseMs?: number;
}

export interface ConsumeOptions<M extends AnySchema | undefined> extends WorkloadPolicies {
  message?: M;
  /** Messages processed at once by this consumer. Default 1. */
  concurrency?: number;
  /** Delivery is at-least-once; declare retry to accept re-delivery. */
  retry?: RetryOptions;
  /** PostgreSQL resource backing the queue. Default: the first declared. */
  database?: PostgresDeclaration;
  errors?: DeclaredError[];
  resources?: ResourceDeclaration[];
}

export interface QueueContext<M> extends BaseContext {
  readonly message: M;
  /** 1-based attempt number. */
  readonly attempt: number;
  readonly id: string;
}

function consume<M extends AnySchema | undefined = undefined>(
  topic: string,
  options: ConsumeOptions<M>,
  handler: (ctx: QueueContext<M extends AnySchema ? Output<M> : unknown>) => unknown,
): Workload {
  const policies: WorkloadPolicies = {};
  if (options.timeout !== undefined) policies.timeout = options.timeout;
  const resources = [...(options.resources ?? [])];
  if (options.database && !resources.some((r) => r.name === options.database!.name)) resources.push(options.database);
  const retry = options.retry
    ? { maxAttempts: options.retry.maxAttempts, backoff: options.retry.backoff ?? "fixed", baseMs: options.retry.baseMs ?? 1000 }
    : undefined;
  return {
    __usai: "workload",
    kind: "queue",
    name: topic,
    trigger: {
      topic,
      concurrency: options.concurrency ?? 1,
      ...(options.database ? { database: options.database.name } : {}),
      ...(retry ? { retry } : {}),
    },
    contracts: options.message ? { message: options.message } : {},
    errors: options.errors ?? [],
    resources,
    dispatches: [],
    policies,
    handler: handler as Workload["handler"],
  };
}

export const queue = { consume };

export interface QueueHandle {
  /** Enqueues a message. Resolves once the insert is durable in the queue's
   * database; processing happens in its own world later. */
  publish(topic: string, message: unknown, options?: { delayMs?: number; database?: PostgresDeclaration }): Promise<{ id: string }>;
}
