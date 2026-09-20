// Authentication is a declared boundary (ADR-0004), reused by reference.

import type {
  AuthDeclaration,
  CredentialLocation,
  ResourceDeclaration,
  ResourcesOf,
} from "./declarations.ts";
import type { BaseContext } from "./runtime/context.ts";

/** What a resolver runs with: the workload's context plus the request's
 * envelope; `resources` typed from the scheme's own `resources: [...]`.
 *
 * @category Authentication
 */
export type ResolverContext<R> = Omit<BaseContext, "resources"> & {
  readonly resources: ResourcesOf<R>;
  readonly request: AuthRequest;
};

/** What an auth resolver sees of the request: method, path, headers and
 * query — never the body. The resolver runs **inside the request's world**,
 * after the boundary validated the request and before the handler
 * (ADR-0004): a session lookup is an ordinary query on the scheme's own
 * `resources: [db]` (typed on `ctx.resources`; every workload that uses the
 * scheme leases them too), and `ctx.env` is the application's.
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
export interface BearerOptions<P, R = ResourceDeclaration[]> {
  /** Scheme name (the OpenAPI security scheme and what Try it remembers). Default `bearer-<n>`. */
  name?: string;
  /** For the OpenAPI security scheme and the reference: where the credential comes from. */
  description?: string;
  /** The resources the resolver leases (a sessions table). Every workload
   * that uses the scheme gets them, in addition to its own `resources`, and
   * `ctx.resources` in `resolve` is typed by them. */
  resources?: R;
  /** Return the principal, or throw `errors.unauthorized()`. */
  resolve: (ctx: ResolverContext<R>, token: string) => P | Promise<P>;
}

/** Options for `auth.header`.
 *
 * @category Authentication
 */
export interface HeaderOptions<P, R = ResourceDeclaration[]> {
  /** Scheme name. Default `header-<n>`. */
  name?: string;
  /** Where the credential comes from, for the reference. */
  description?: string;
  /** The header carrying the credential (case-insensitive). */
  header: string;
  /** The resources the resolver leases; every workload using the scheme gets them. */
  resources?: R;
  /** Return the principal, or throw `errors.unauthorized()`. */
  resolve: (ctx: ResolverContext<R>, value: string) => P | Promise<P>;
}

/** Options for `auth.cookie`.
 *
 * @category Authentication
 */
export interface CookieOptions<P, R = ResourceDeclaration[]> {
  name?: string;
  description?: string;
  /** The cookie's name (`sid`). Its value is what `resolve` receives; a
   * signed value is verified in the resolver with `cookies.verify`. */
  cookie: string;
  /** The resources the resolver leases; every workload using the scheme gets them. */
  resources?: R;
  /** Return the principal, or throw `errors.unauthorized()`. */
  resolve: (ctx: ResolverContext<R>, value: string) => P | Promise<P>;
}

/** Options for `auth.custom`.
 *
 * @category Authentication
 */
export interface CustomOptions<P, R = ResourceDeclaration[]> {
  name: string;
  /** Where the credential comes from, for the reference. */
  description?: string;
  /** Where the credential travels — `{ in: "cookie", name: "sid" }` for a
   * session cookie. Declares it for the OpenAPI document (`apiKey` in that
   * location) and the reference's request panel; the resolver still reads
   * the request itself. Without it the document does not invent a header:
   * the operation is marked authenticated with a custom scheme and no
   * security scheme is emitted. */
  credential?: CredentialLocation;
  /** The resources the resolver leases; every workload using the scheme gets them. */
  resources?: R;
  /** Inspect `ctx.request` (headers, query) and return the principal, or throw. */
  resolve: (ctx: ResolverContext<R>) => P | Promise<P>;
}

let anonymous = 0;

/**
 * Declare an authentication boundary. Attach it to a workload with
 * `auth: <declaration>`; `resolve` runs before the handler, with the
 * workload's `ctx` — its declared resources plus the scheme's own
 * `resources`, typed — and `ctx.request`, and the
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
 * export const session = auth.bearer({
 *   name: "session",
 *   description: "The token from POST /login",
 *   resources: [db],                       // the resolver's own; every route using the scheme gets them
 *   resolve: async (ctx, token) => {
 *     const row = await ctx.resources.db.one<Principal>("select … from sessions where token = $1", [token]);
 *     if (!row) throw errors.unauthorized("unknown or expired token");
 *     return row;
 *   },
 * });
 * export const me = http.get("/me", { auth: session }, async (ctx) => ctx.auth);
 * ```
 *
 * @category Authentication
 */
export const auth = {
  /** `Authorization: Bearer <token>`; `resolve` receives the token. */
  bearer<P, const R extends readonly ResourceDeclaration[] = ResourceDeclaration[]>(
    options: BearerOptions<P, R>,
  ): AuthDeclaration<P, R> {
    return {
      __usai: "auth",
      name: options.name ?? `bearer-${++anonymous}`,
      ...(options.description ? { description: options.description } : {}),
      scheme: "bearer",
      header: "authorization",
      resources: (options.resources ?? []) as R,
      resolve: options.resolve as AuthDeclaration<P, R>["resolve"],
    };
  },
  /** A credential in the named header (an API key); `resolve` receives its value. */
  header<P, const R extends readonly ResourceDeclaration[] = ResourceDeclaration[]>(
    options: HeaderOptions<P, R>,
  ): AuthDeclaration<P, R> {
    return {
      __usai: "auth",
      name: options.name ?? `header-${++anonymous}`,
      ...(options.description ? { description: options.description } : {}),
      scheme: "header",
      header: options.header.toLowerCase(),
      resources: (options.resources ?? []) as R,
      resolve: options.resolve as AuthDeclaration<P, R>["resolve"],
    };
  },
  /** A cookie (`cookie: "sid"`); `resolve` receives its value. A missing
   * cookie is a 401 before the handler runs. Login sets it with
   * `http.response(200, body, { "set-cookie": cookies.serialize("sid", value, { maxAge }) })`,
   * logout clears it with `maxAge: 0`. The OpenAPI document says
   * `apiKey in: cookie` and the reference's request panel sends the browser's
   * cookie with `credentials: include`. */
  cookie<P, const R extends readonly ResourceDeclaration[] = ResourceDeclaration[]>(
    options: CookieOptions<P, R>,
  ): AuthDeclaration<P, R> {
    return {
      __usai: "auth",
      name: options.name ?? `cookie-${++anonymous}`,
      ...(options.description ? { description: options.description } : {}),
      scheme: "cookie",
      credential: { in: "cookie", name: options.cookie },
      resources: (options.resources ?? []) as R,
      resolve: options.resolve as AuthDeclaration<P, R>["resolve"],
    };
  },
  /** Anything else: `resolve` reads the request itself. */
  custom<P, const R extends readonly ResourceDeclaration[] = ResourceDeclaration[]>(
    options: CustomOptions<P, R>,
  ): AuthDeclaration<P, R> {
    return {
      __usai: "auth",
      name: options.name,
      ...(options.description ? { description: options.description } : {}),
      scheme: "custom",
      ...(options.credential ? { credential: options.credential } : {}),
      resources: (options.resources ?? []) as R,
      resolve: ((ctx: ResolverContext<R>) => options.resolve(ctx)) as AuthDeclaration<
        P,
        R
      >["resolve"],
    };
  },
};
