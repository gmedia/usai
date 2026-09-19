import { auth, defineModule, errors, http, password, type PostgresHandle } from "@sakaladev/usai";
import { z } from "zod";
import { db } from "../resources.ts";

const sql = (ctx: { resources: Record<string, unknown> }) =>
  ctx.resources["main"] as PostgresHandle;

export interface Principal {
  userId: string;
  tenantId: string;
  email: string;
}

const Credentials = z.object({
  email: z.string().email().max(200),
  password: z.string().min(12).max(200),
});
const Signup = Credentials.extend({
  tenant: z.string().regex(/^[a-z0-9-]{2,40}$/),
  /** The tenant's display name (defaults to the slug); returned by GET /me as `tenantName`. */
  name: z.string().min(1).max(120).optional(),
});
const Session = z.object({ token: z.string(), expiresAt: z.string() });
const Me = z.object({
  userId: z.string().uuid(),
  tenantId: z.string().uuid(),
  email: z.string(),
  tenant: z.string(),
  tenantName: z.string(),
});
// http(s) only: `z.string().url()` alone accepts any scheme (javascript:, file:).
const Webhook = z.object({
  url: z
    .string()
    .url()
    .max(500)
    .regex(/^https?:\/\//, "http(s) URL"),
  secret: z.string().min(16).max(200),
});
const WebhookSet = z.object({ url: z.string() });

const hex = (bytes: ArrayBuffer | Uint8Array) =>
  Array.from(bytes instanceof Uint8Array ? bytes : new Uint8Array(bytes), (b) =>
    b.toString(16).padStart(2, "0"),
  ).join("");
const sha256 = async (text: string) =>
  hex(await crypto.subtle.digest("SHA-256", new TextEncoder().encode(text)));

/** Bearer sessions: the token is random from host entropy; only its SHA-256
 * is stored. The resolver runs inside the request's world with the
 * workload's resources (ADR-0004). */
export const session = auth.bearer<Principal>({
  name: "session",
  description: "The token from POST /signup or POST /login; expires after SESSION_TTL_HOURS.",
  resolve: async (ctx, token) => {
    const row = await sql(ctx).one<Principal>(
      `select u.id as "userId", u.tenant_id as "tenantId", u.email
         from sessions s join users u on u.id = s.user_id
        where s.token_hash = $1 and s.expires_at > now()`,
      [await sha256(token)],
    );
    if (!row) throw errors.unauthorized("invalid or expired session");
    return row;
  },
});

async function openSession(
  ctx: { resources: Record<string, unknown>; env: Record<string, unknown> },
  userId: string,
) {
  const token = hex(crypto.getRandomValues(new Uint8Array(32)));
  const ttlHours =
    typeof ctx.env["SESSION_TTL_HOURS"] === "number" ? ctx.env["SESSION_TTL_HOURS"] : 24 * 7;
  const row = await sql(ctx).one<{ expiresAt: string }>(
    `insert into sessions (token_hash, user_id, expires_at) values ($1, $2, now() + ($3::int * interval '1 hour')) returning expires_at as "expiresAt"`,
    [await sha256(token), userId, ttlHours],
  );
  return { token, expiresAt: row!.expiresAt };
}

export const signup = http.post(
  "/signup",
  {
    body: Signup,
    response: { 201: Session },
    errors: [{ code: "conflict", status: 409 }],
    resources: [db],
  },
  async (ctx) => {
    const hash = await password.hash(ctx.body.password);
    // Tenant and first user are one unit: no tenant without an owner.
    const userId = await sql(ctx).transaction(async (tx) => {
      const exists = await tx.one(`select 1 from tenants where slug = $1`, [ctx.body.tenant]);
      if (exists) throw errors.conflict(`tenant ${ctx.body.tenant} already exists`);
      const tenant = await tx.one<{ id: string }>(
        `insert into tenants (slug, name) values ($1, $2) returning id`,
        [ctx.body.tenant, ctx.body.name ?? ctx.body.tenant],
      );
      const user = await tx.one<{ id: string }>(
        `insert into users (tenant_id, email, password_hash) values ($1, $2, $3) returning id`,
        [tenant!.id, ctx.body.email.toLowerCase(), hash],
      );
      return user!.id;
    });
    return http.created(await openSession(ctx, userId));
  },
);

export const login = http.post(
  "/login",
  {
    body: Credentials.extend({ tenant: z.string() }),
    response: { 200: Session },
    errors: [{ code: "unauthorized", status: 401 }],
    resources: [db],
  },
  async (ctx) => {
    const user = await sql(ctx).one<{ id: string; passwordHash: string }>(
      `select u.id, u.password_hash as "passwordHash" from users u join tenants t on t.id = u.tenant_id where t.slug = $1 and u.email = $2`,
      [ctx.body.tenant, ctx.body.email.toLowerCase()],
    );
    // Same answer and roughly the same cost whether the user exists or not.
    const ok = user
      ? await password.verify(ctx.body.password, user.passwordHash)
      : await password.verify(ctx.body.password, await password.hash("unused"));
    if (!user || !ok) throw errors.unauthorized("wrong tenant, email or password");
    return openSession(ctx, user.id);
  },
);

export const logout = http.post(
  "/logout",
  { auth: session, response: { 204: z.null() }, resources: [db] },
  async (ctx) => {
    const raw = /^Bearer\s+(.+)$/i.exec(ctx.headers["authorization"] ?? "")?.[1]?.trim() ?? "";
    await sql(ctx).execute(`delete from sessions where token_hash = $1`, [await sha256(raw)]);
    return http.noContent();
  },
);

export const me = http.get(
  "/me",
  { auth: session, response: { 200: Me }, resources: [db] },
  async (ctx) => {
    const row = await sql(ctx).one<{ slug: string; name: string }>(
      `select slug, name from tenants where id = $1`,
      [ctx.auth.tenantId],
    );
    return { ...ctx.auth, tenant: row!.slug, tenantName: row!.name };
  },
);

export const setWebhook = http.put(
  "/tenant/webhook",
  { auth: session, body: Webhook, response: { 200: WebhookSet }, resources: [db] },
  async (ctx) => {
    await sql(ctx).execute(
      `update tenants set webhook_url = $1, webhook_secret = $2 where id = $3`,
      [ctx.body.url, ctx.body.secret, ctx.auth.tenantId],
    );
    return { url: ctx.body.url };
  },
);

export const authModule = defineModule({
  name: "auth",
  workloads: [signup, login, logout, me, setWebhook],
  resources: [db],
  migrations: "./src/auth/migrations/*.sql",
});
