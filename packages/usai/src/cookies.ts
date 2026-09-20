// Cookies at the boundary: parsing the `cookie` header a browser sends,
// serialising a `Set-Cookie` value, and signing a value so a cookie the
// client can read cannot be forged. Pure functions plus `crypto.subtle`
// (present in every world); no state, no store — a session store is a
// table, and `auth.cookie` is the scheme that hands the cookie's value to
// the resolver.

/** Attributes of a `Set-Cookie` value. Secure, HttpOnly and `SameSite=Lax`
 * are the defaults: a session cookie that a script can read or that travels
 * over http is the exception, and has to be asked for.
 *
 * @category Authentication
 */
export interface CookieAttributes {
  /** Seconds until expiry; `0` deletes the cookie (`Max-Age=0`). */
  maxAge?: number;
  expires?: Date;
  /** Default `/`. */
  path?: string;
  domain?: string;
  /** Default `true`. */
  secure?: boolean;
  /** Default `true`. */
  httpOnly?: boolean;
  /** Default `"Lax"`. `"None"` requires `secure`. */
  sameSite?: "Strict" | "Lax" | "None";
}

const NAME = /^[!#$%&'*+\-.^_`|~0-9A-Za-z]+$/;

/** Parses a `cookie` request header into a name → value map (the first
 * occurrence of a name wins, as browsers order them by specificity).
 * Values are percent-decoded when they decode; a malformed pair is skipped.
 *
 * @category Authentication
 */
export function parseCookies(header: string | undefined | null): Record<string, string> {
  const out: Record<string, string> = {};
  if (!header) return out;
  for (const part of header.split(";")) {
    const eq = part.indexOf("=");
    if (eq < 0) continue;
    const name = part.slice(0, eq).trim();
    if (!name || name in out) continue;
    let value = part.slice(eq + 1).trim();
    if (value.startsWith('"') && value.endsWith('"') && value.length >= 2)
      value = value.slice(1, -1);
    try {
      out[name] = decodeURIComponent(value);
    } catch {
      out[name] = value;
    }
  }
  return out;
}

/** Serialises one `Set-Cookie` header value. The value is percent-encoded
 * where the cookie grammar requires it, so anything round-trips through
 * `parseCookies`.
 *
 * @example
 * ```ts
 * http.response(200, body, { "set-cookie": serializeCookie("sid", token, { maxAge: 86_400 }) });
 * http.noContent({ "set-cookie": serializeCookie("sid", "", { maxAge: 0 }) });   // logout
 * ```
 *
 * @category Authentication
 */
export function serializeCookie(
  name: string,
  value: string,
  attributes: CookieAttributes = {},
): string {
  if (!NAME.test(name)) throw new TypeError(`invalid cookie name: ${JSON.stringify(name)}`);
  const parts = [`${name}=${encodeURIComponent(value)}`];
  const {
    maxAge,
    expires,
    path = "/",
    domain,
    secure = true,
    httpOnly = true,
    sameSite = "Lax",
  } = attributes;
  if (maxAge !== undefined) {
    if (!Number.isInteger(maxAge) || maxAge < 0)
      throw new TypeError("maxAge must be a non-negative integer");
    parts.push(`Max-Age=${maxAge}`);
  }
  if (expires) parts.push(`Expires=${expires.toUTCString()}`);
  if (domain) parts.push(`Domain=${domain}`);
  parts.push(`Path=${path}`);
  if (sameSite === "None" && !secure) throw new TypeError("SameSite=None requires secure");
  if (secure) parts.push("Secure");
  if (httpOnly) parts.push("HttpOnly");
  parts.push(`SameSite=${sameSite}`);
  return parts.join("; ");
}

const encoder = new TextEncoder();
const b64url = (bytes: ArrayBuffer): string =>
  btoa(String.fromCharCode(...new Uint8Array(bytes)))
    .replace(/\+/g, "-")
    .replace(/\//g, "_")
    .replace(/=+$/, "");

async function hmac(secret: string, value: string): Promise<string> {
  const key = await crypto.subtle.importKey(
    "raw",
    encoder.encode(secret),
    { name: "HMAC", hash: "SHA-256" },
    false,
    ["sign"],
  );
  return b64url(await crypto.subtle.sign("HMAC", key, encoder.encode(value)));
}

/** `value.signature` — a value the client can read but not alter. The
 * signature is HMAC-SHA256 over the value with `secret`, base64url. Rotate
 * by verifying against several secrets and signing with the newest.
 *
 * @category Authentication
 */
export async function signCookieValue(value: string, secret: string): Promise<string> {
  if (!secret) throw new TypeError("a cookie secret is required");
  // Any value: the signature is base64url (no dot), and verify splits on
  // the last dot, so `user-id|email` or a dotted id round-trips.
  return `${value}.${await hmac(secret, value)}`;
}

/** The value behind a `signCookieValue` result, or `null` when the
 * signature does not match any of the secrets (constant-time compare per
 * secret).
 *
 * @category Authentication
 */
export async function verifyCookieValue(
  signed: string | undefined | null,
  ...secrets: string[]
): Promise<string | null> {
  if (!signed) return null;
  const dot = signed.lastIndexOf(".");
  if (dot <= 0) return null;
  const value = signed.slice(0, dot);
  const signature = signed.slice(dot + 1);
  for (const secret of secrets) {
    const expected = await hmac(secret, value);
    if (expected.length === signature.length) {
      let diff = 0;
      for (let i = 0; i < expected.length; i++)
        diff |= expected.charCodeAt(i) ^ signature.charCodeAt(i);
      if (diff === 0) return value;
    }
  }
  return null;
}

/** The cookie helpers as one object, for `import { cookies } from "@sakaladev/usai"`.
 *
 * @category Authentication
 */
export const cookies = {
  parse: parseCookies,
  serialize: serializeCookie,
  sign: signCookieValue,
  verify: verifyCookieValue,
};
