// Signed, self-describing tokens for bearer access tokens: a JSON payload
// with an expiry, HMAC-SHA256 over it, both base64url — the shape a mobile
// client stores and sends back, verified without a database hit. Not a JWT
// (no header, no algorithm negotiation, no third-party verification — the
// world has HMAC only, `GUIDE.md` §12); a session store or a refresh token
// stays a table row, and `cookies.sign` stays the cookie form.

import { bytes } from "./bytes.ts";
import { parseDurationMs } from "./runtime/context.ts";

/** Options for `tokens.sign`.
 *
 * @category Authentication
 */
export interface TokenOptions {
  /** Lifetime: `"15m"`, `"12h"`, or seconds as a number. Required — a
   * bearer token without an expiry is a password. */
  expiresIn: string | number;
  /** `iat`/`exp` are computed from this instant (tests). Default `Date.now()`. */
  now?: number;
}

/** What `tokens.verify` returns: the payload as signed, plus the claims
 * `sign` added.
 *
 * @category Authentication
 */
export type TokenClaims<T> = T & {
  /** Unix seconds the token was issued. */
  iat: number;
  /** Unix seconds the token expires (exclusive). */
  exp: number;
};

const encoder = new TextEncoder();
const decoder = new TextDecoder();

function b64url(data: Uint8Array): string {
  return bytes.toBase64(data).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

function unb64url(text: string): Uint8Array | null {
  if (!/^[A-Za-z0-9_-]*$/.test(text)) return null;
  const padded =
    text.replace(/-/g, "+").replace(/_/g, "/") + "=".repeat((4 - (text.length % 4)) % 4);
  try {
    return bytes.fromBase64(padded);
  } catch {
    return null;
  }
}

async function hmac(secret: string, data: string): Promise<Uint8Array> {
  const key = await crypto.subtle.importKey(
    "raw",
    encoder.encode(secret),
    { name: "HMAC", hash: "SHA-256" },
    false,
    ["sign"],
  );
  // `Uint8Array<ArrayBuffer>` for the DOM lib's `BufferSource` (the typedoc
  // pass compiles against it; a world's globals are looser).
  return new Uint8Array(
    await crypto.subtle.sign("HMAC", key, encoder.encode(data) as Uint8Array<ArrayBuffer>),
  );
}

/** Signs `payload` (a JSON object of your claims — a user id, a role) with
 * `secret`, adding `iat` and `exp`. The result is `<payload>.<signature>`,
 * both base64url, opaque to the client, verifiable by any instance that
 * holds the secret.
 *
 * @example
 * ```ts
 * const access = await tokens.sign({ sub: user.id, role: user.role }, ctx.env.ACCESS_TOKEN_SECRET, { expiresIn: "15m" });
 * ```
 *
 * @category Authentication
 */
export async function signToken<T extends Record<string, unknown>>(
  payload: T,
  secret: string,
  options: TokenOptions,
): Promise<string> {
  if (!secret) throw new TypeError("a token secret is required");
  if ("iat" in payload || "exp" in payload)
    throw new TypeError("iat and exp are set by sign; do not put them in the payload");
  const ttlMs = parseDurationMs(options.expiresIn);
  if (!(ttlMs > 0)) throw new TypeError("expiresIn must be positive");
  const now = options.now ?? Date.now();
  const iat = Math.floor(now / 1000);
  const claims: TokenClaims<T> = { ...payload, iat, exp: Math.floor((now + ttlMs) / 1000) };
  const body = b64url(encoder.encode(JSON.stringify(claims)));
  const signature = b64url(await hmac(secret, body));
  return `${body}.${signature}`;
}

/** The claims of a token `signToken` produced, or `null` when the token is
 * malformed, expired, or signed with none of the secrets (pass an array to
 * rotate: verify against old and new, sign with the new). Comparison is
 * constant-time per secret. Nothing about *why* it failed is returned — a
 * client gets `401 unauthorized` either way, and the reason is not its
 * business.
 *
 * @example
 * ```ts
 * const userBearer = auth.bearer({
 *   name: "user",
 *   resolve: async (ctx, token) => {
 *     const claims = await tokens.verify<{ sub: string }>(token, String(ctx.env.ACCESS_TOKEN_SECRET));
 *     if (!claims) throw errors.unauthorized();
 *     return { userId: claims.sub };
 *   },
 * });
 * ```
 *
 * @category Authentication
 */
export async function verifyToken<T extends Record<string, unknown> = Record<string, unknown>>(
  token: string | undefined | null,
  secrets: string | readonly string[],
  options: { now?: number } = {},
): Promise<TokenClaims<T> | null> {
  if (!token || token.length > 8192) return null;
  const candidates = (typeof secrets === "string" ? [secrets] : secrets).filter(
    (s) => s.length > 0,
  );
  if (candidates.length === 0) throw new TypeError("a token secret is required");
  const dot = token.indexOf(".");
  if (dot <= 0 || dot === token.length - 1 || token.indexOf(".", dot + 1) !== -1) return null;
  const body = token.slice(0, dot);
  const presented = unb64url(token.slice(dot + 1));
  if (!presented || presented.length !== 32) return null;
  let valid = false;
  for (const secret of candidates) {
    const expected = await hmac(secret, body);
    let diff = 0;
    for (let i = 0; i < 32; i++) diff |= (expected[i] ?? 0) ^ (presented[i] ?? 0);
    if (diff === 0) valid = true;
  }
  if (!valid) return null;
  const raw = unb64url(body);
  if (!raw) return null;
  let claims: unknown;
  try {
    claims = JSON.parse(decoder.decode(raw));
  } catch {
    return null;
  }
  if (typeof claims !== "object" || claims === null || Array.isArray(claims)) return null;
  const { exp, iat } = claims as { exp?: unknown; iat?: unknown };
  if (typeof exp !== "number" || typeof iat !== "number") return null;
  if (Math.floor((options.now ?? Date.now()) / 1000) >= exp) return null;
  return claims as TokenClaims<T>;
}

/** The token helpers as one object, for `import { tokens } from "@sakaladev/usai"`.
 *
 * @category Authentication
 */
export const tokens = { sign: signToken, verify: verifyToken };
