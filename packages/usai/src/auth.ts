// Authentication is a declared boundary (ADR-0004), reused by reference.

import type { AuthDeclaration } from "./declarations.ts";
import type { BaseContext } from "./runtime/context.ts";

/** What an auth resolver sees of the request: no body, no world yet.
 *
 * @category Authentication
 */
export interface AuthRequest {
  readonly method: string;
  readonly path: string;
  readonly headers: Record<string, string>;
  readonly query: Record<string, string | string[]>;
}

/** Options for `auth.bearer`.
 *
 * @category Authentication
 */
export interface BearerOptions<P> {
  /** Scheme name (the OpenAPI security scheme and what Try it remembers). Default `bearer-<n>`. */
  name?: string;
  /** For the OpenAPI security scheme and the reference: where the credential comes from. */
  description?: string;
  /** Return the principal, or throw `errors.unauthorized()`. */
  resolve: (ctx: BaseContext & { request: AuthRequest }, token: string) => P | Promise<P>;
}

/** Options for `auth.header`.
 *
 * @category Authentication
 */
export interface HeaderOptions<P> {
  /** Scheme name. Default `header-<n>`. */
  name?: string;
  /** Where the credential comes from, for the reference. */
  description?: string;
  /** The header carrying the credential (case-insensitive). */
  header: string;
  /** Return the principal, or throw `errors.unauthorized()`. */
  resolve: (ctx: BaseContext & { request: AuthRequest }, value: string) => P | Promise<P>;
}

/** Options for `auth.custom`.
 *
 * @category Authentication
 */
export interface CustomOptions<P> {
  name: string;
  /** Where the credential comes from, for the reference. */
  description?: string;
  /** Inspect `ctx.request` (headers, query) and return the principal, or throw. */
  resolve: (ctx: BaseContext & { request: AuthRequest }) => P | Promise<P>;
}

let anonymous = 0;

/**
 * Declare an authentication boundary. Attach it to a workload with
 * `auth: <declaration>`; `resolve` runs before the handler, with the
 * workload's `ctx` (its declared resources) plus `ctx.request`, and the
 * principal it returns is `ctx.auth`, typed. A missing credential or a
 * thrown `errors.unauthorized()` answers 401 and the handler never runs.
 * Authentication (who) lives here; authorization (may they) is business
 * logic in the handler. One declaration is reused by reference across
 * endpoints; its `name` is the OpenAPI security scheme. In v0 the resolver
 * is application code and runs inside the request's world (ADR-0004);
 * the rest of the boundary — routing, decoding, schema validation — runs
 * before any world exists.
 *
 * @example
 * ```ts
 * export const session = auth.bearer<Principal>({
 *   name: "session",
 *   description: "The token from POST /login",
 *   resolve: async (ctx, token) => {
 *     // The resolver's ctx carries the workload's resources, untyped: cast to the handle.
 *     const row = await (ctx.resources.db as PostgresHandle).one<Principal>("select … from sessions where token = $1", [token]);
 *     if (!row) throw errors.unauthorized("unknown or expired token");
 *     return row;
 *   },
 * });
 * export const me = http.get("/me", { auth: session, resources: [db] }, async (ctx) => ctx.auth);
 * ```
 *
 * @category Authentication
 */
export const auth = {
  /** `Authorization: Bearer <token>`; `resolve` receives the token. */
  bearer<P>(options: BearerOptions<P>): AuthDeclaration<P> {
    return {
      __usai: "auth",
      name: options.name ?? `bearer-${++anonymous}`,
      ...(options.description ? { description: options.description } : {}),
      scheme: "bearer",
      header: "authorization",
      resolve: options.resolve as AuthDeclaration<P>["resolve"],
    };
  },
  /** A credential in the named header (an API key); `resolve` receives its value. */
  header<P>(options: HeaderOptions<P>): AuthDeclaration<P> {
    return {
      __usai: "auth",
      name: options.name ?? `header-${++anonymous}`,
      ...(options.description ? { description: options.description } : {}),
      scheme: "header",
      header: options.header.toLowerCase(),
      resolve: options.resolve as AuthDeclaration<P>["resolve"],
    };
  },
  /** Anything else: `resolve` reads the request itself. */
  custom<P>(options: CustomOptions<P>): AuthDeclaration<P> {
    return {
      __usai: "auth",
      name: options.name,
      ...(options.description ? { description: options.description } : {}),
      scheme: "custom",
      resolve: ((ctx: BaseContext & { request: AuthRequest }) => options.resolve(ctx)) as AuthDeclaration<P>["resolve"],
    };
  },
};
