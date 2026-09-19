// Queue / message workloads (`GOAL.md` §19). Persistent consumer
// infrastructure, a fresh world per message, explicit retry (ADR-0014).

import type { AnySchema, Output } from "./schema.ts";
import type { DeclaredError, ResourceDeclaration, ResourcesOf, Workload, WorkloadPolicies } from "./declarations.ts";
import type { PostgresDeclaration } from "./resources.ts";
import type { BaseContext } from "./runtime/context.ts";

/** Retry policy of a queue consumer.
 *
 * @category Queues
 */
export interface RetryOptions {
  /** Total attempts including the first. Default 1: failure is terminal. */
  maxAttempts: number;
  /** `fixed` (default): `baseMs` between attempts; `exponential`: doubling from `baseMs`. */
  backoff?: "fixed" | "exponential";
  /** Base delay in ms (default 1000). */
  baseMs?: number;
}

/** Options for `queue.consume`.
 *
 * @category Queues
 */
export interface ConsumeOptions<M extends AnySchema | undefined, R extends ResourceDeclaration[] = ResourceDeclaration[]> extends WorkloadPolicies {
  /** A paragraph for the reference. */
  description?: string;
  /** Schema for the message; validated before the message's world exists.
   * A message that fails validation is dead-lettered, not retried. */
  message?: M;
  /** Messages processed at once by this consumer. Default 1. */
  concurrency?: number;
  /** Delivery is at-least-once; declare retry to accept re-delivery. */
  retry?: RetryOptions;
  /** PostgreSQL resource holding the `usai_queue` table. Default: the
   * first `postgres` resource declared in the application (declaration
   * order: app-level resources, then modules in order). Publishers use the
   * same default, so one database serves every topic unless both sides
   * name another. */
  database?: PostgresDeclaration;
  errors?: DeclaredError[];
  /** Resources the message's world leases; `ctx.resources` is typed from
   * this list. The queue's own database need not be listed. */
  resources?: R;
}

/** The context of one message delivery: the validated `message`, the
 * `attempt` number and the message `id`, plus {@link BaseContext}.
 *
 * @category Queues
 */
export interface QueueContext<M, R = ResourceDeclaration[]> extends BaseContext {
  readonly message: M;
  readonly resources: ResourcesOf<R>;
  /** 1-based attempt number. */
  readonly attempt: number;
  /** The queue's id for this message (the one `publish` returned). */
  readonly id: string;
}

/**
 * Declare a consumer for a topic. The queue lives in PostgreSQL (the
 * `usai_queue` table of `database`, created by the runtime), so a message
 * survives restarts and is delivered **at least once**. Each message runs
 * in a **fresh world**, `concurrency` of them at a time; the world
 * committing acknowledges the message. A throw schedules the next attempt
 * per `retry` and, after the last one, moves the message to the dead
 * letter state with the error, where `usai_queue` keeps it for an
 * operator. The consumer itself is persistent infrastructure owned by the
 * revision: it starts at activation and stops at drain.
 *
 * Producers call `ctx.queue.publish(topic, message)` from any workload and
 * declare it with {@link publishes} so the reference page links the two.
 *
 * @param topic The topic name; also the workload name (`queue:<topic>`).
 *
 * @example
 * ```ts
 * export const deliver = queue.consume(
 *   "webhook.deliver",
 *   { message: WebhookEvent, concurrency: 4, retry: { maxAttempts: 5, backoff: "exponential", baseMs: 500 }, resources: [db, hooks] },
 *   async (ctx) => {
 *     const res = await ctx.resources.hooks.fetch(ctx.message.url, { method: "POST", json: ctx.message });
 *     if (!res.ok) throw errors.unavailable(`endpoint answered ${res.status}`); // retried, then dead-lettered
 *   },
 * );
 * ```
 */
function consume<M extends AnySchema | undefined = undefined, R extends ResourceDeclaration[] = ResourceDeclaration[]>(
  topic: string,
  options: ConsumeOptions<M, R>,
  handler: (ctx: QueueContext<M extends AnySchema ? Output<M> : unknown, R>) => unknown,
): Workload {
  const policies: WorkloadPolicies = {};
  if (options.timeout !== undefined) policies.timeout = options.timeout;
  const resources: ResourceDeclaration[] = [...(options.resources ?? [])];
  if (options.database && !resources.some((r) => r.name === options.database!.name)) resources.push(options.database);
  const retry = options.retry
    ? { maxAttempts: options.retry.maxAttempts, backoff: options.retry.backoff ?? "fixed", baseMs: options.retry.baseMs ?? 1000 }
    : undefined;
  return {
    __usai: "workload",
    kind: "queue",
    name: topic,
    ...(options.description ? { description: options.description } : {}),
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
    publishes: [],
    policies,
    handler: handler as Workload["handler"],
  };
}

/** Queue workloads: `queue.consume(topic, options, handler)`.
 *
 * @category Queues
 */
export const queue = { consume };

/** `ctx.queue` in every world.
 *
 * @category Queues
 */
export interface QueueHandle {
  /** Enqueue a message for the topic's consumer. Resolves with the
   * message id once the row is durable in the queue's database (the
   * consumer's `database`, by default the application's first `postgres`
   * resource — the publishing workload need not declare it); processing
   * happens later, in the consumer's own world, at least once. The message
   * must satisfy the consumer's `message` schema or it is dead-lettered on
   * arrival, so adding a new event means extending that schema first.
   * Declare the edge with {@link publishes} so the reference links the two.
   * `delayMs` holds the message back; `database` targets another queue. */
  publish(topic: string, message: unknown, options?: { delayMs?: number; database?: PostgresDeclaration }): Promise<{ id: string }>;
}
