// Application errors are contracts (`GOAL.md` §14, contract C11).

/** The wire shape of an application error: `{ "error": { code, message, details? } }`.
 *
 * @category Errors
 */
export interface UsaiErrorShape {
  code: string;
  status: number;
  details?: unknown;
}

/** An error with a `code` and an HTTP `status`. Thrown from a handler it
 * becomes the response `{ "error": { code, message, details? } }` with
 * that status; from a task or queue message it is the outcome's error.
 * Any other thrown value is a 500 `internal` with a sanitized message.
 *
 * @category Errors
 */
export class UsaiError extends Error {
  readonly usai: UsaiErrorShape;
  constructor(code: string, status: number, message: string, details?: unknown) {
    super(message);
    this.name = "UsaiError";
    this.usai = details === undefined ? { code, status } : { code, status, details };
  }
}

/** Whether a caught value is a {@link UsaiError} (also across module copies).
 *
 * @category Errors
 */
export function isUsaiError(value: unknown): value is UsaiError {
  return value instanceof Error && typeof (value as UsaiError).usai === "object" && (value as UsaiError).usai !== null;
}

function make(code: string, status: number) {
  return (message?: string, details?: unknown): UsaiError => new UsaiError(code, status, message ?? code.replace(/_/g, " "), details);
}

/**
 * Constructors for the common {@link UsaiError}s. Each takes an optional
 * message (default: the code, spaced) and `details` (any JSON, echoed to
 * the client — keep it safe to show). List the codes a workload throws in
 * its `errors` option so the reference and the OpenAPI document say so.
 *
 * @example
 * ```ts
 * const invoice = await ctx.resources.db.one("select … where id = $1", [ctx.params.id]);
 * if (!invoice) throw errors.notFound("invoice not found", { id: ctx.params.id });
 * if (invoice.status !== "draft") throw errors.conflict("only a draft can be issued");
 * ```
 *
 * @category Errors
 */
export const errors = {
  /** 400 `bad_request`. */
  badRequest: make("bad_request", 400),
  /** 401 `unauthorized` (what an auth resolver throws). */
  unauthorized: make("unauthorized", 401),
  /** 403 `forbidden`. */
  forbidden: make("forbidden", 403),
  /** 404 `not_found`. */
  notFound: make("not_found", 404),
  /** 409 `conflict`. */
  conflict: make("conflict", 409),
  /** 422 `unprocessable`. */
  unprocessable: make("unprocessable", 422),
  /** 429 `too_many_requests`. */
  tooManyRequests: make("too_many_requests", 429),
  /** 500 `internal`. */
  internal: make("internal", 500),
  /** 503 `unavailable` (a dependency is down; retryable). */
  unavailable: make("unavailable", 503),
  /** A custom declared error. `code` should also be listed in the workload's `errors`. */
  custom(code: string, status: number, message?: string, details?: unknown): UsaiError {
    return new UsaiError(code, status, message ?? code, details);
  },
};
