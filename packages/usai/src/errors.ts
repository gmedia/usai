// Application errors are contracts (`GOAL.md` §14, contract C11).

export interface UsaiErrorShape {
  code: string;
  status: number;
  details?: unknown;
}

export class UsaiError extends Error {
  readonly usai: UsaiErrorShape;
  constructor(code: string, status: number, message: string, details?: unknown) {
    super(message);
    this.name = "UsaiError";
    this.usai = details === undefined ? { code, status } : { code, status, details };
  }
}

export function isUsaiError(value: unknown): value is UsaiError {
  return value instanceof Error && typeof (value as UsaiError).usai === "object" && (value as UsaiError).usai !== null;
}

function make(code: string, status: number) {
  return (message?: string, details?: unknown): UsaiError => new UsaiError(code, status, message ?? code.replace(/_/g, " "), details);
}

export const errors = {
  badRequest: make("bad_request", 400),
  unauthorized: make("unauthorized", 401),
  forbidden: make("forbidden", 403),
  notFound: make("not_found", 404),
  conflict: make("conflict", 409),
  unprocessable: make("unprocessable", 422),
  tooManyRequests: make("too_many_requests", 429),
  internal: make("internal", 500),
  unavailable: make("unavailable", 503),
  /** A custom declared error. `code` should also be listed in the workload's `errors`. */
  custom(code: string, status: number, message?: string, details?: unknown): UsaiError {
    return new UsaiError(code, status, message ?? code, details);
  },
};
