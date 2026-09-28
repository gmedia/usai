// Declared environment requirements (`GOAL.md` §32). Values are resolved by
// the host at activation; missing required values fail activation, not the
// first request.

/** The kinds an environment field can have; `secret` is never printed by `inspect`.
 *
 * @category Environment
 */
export type EnvKind = "string" | "url" | "secret" | "int" | "bool" | "enum" | "cidr" | "list";

/** What a parsed environment value can be: the scalars, an `env.list`'s
 * array of them, or `undefined` for an absent optional.
 *
 * @category Environment
 */
export type EnvValue =
  | string
  | number
  | boolean
  | readonly (string | number | boolean)[]
  | undefined;

/** One declared variable: kind, whether it is required, and its parser.
 *
 * @category Environment
 */
export interface EnvField<T> {
  readonly __usai: "env-field";
  readonly kind: EnvKind;
  readonly required: boolean;
  readonly values?: readonly string[];
  /** `list` only: the kind each item must be, so the **host** can check them
   * at activation. A parser that only runs in a world cannot fail a
   * deployment, which is the whole point of declaring the variable. */
  readonly items?: EnvKind;
  /** `list` only: what separates the items (default `,`). */
  readonly separator?: string;
  /** The group this field belongs to (`env.group`). Every field of one
   * group must be set together or not at all, checked by the **host** at
   * activation. It lives on the field rather than on the declaration because
   * a field map is routinely spread into a larger one
   * (`env({ ...authEnv.fields })`), and a rule attached to the declaration
   * would vanish there without a word. */
  readonly group?: string;
  readonly parse: (raw: string) => T;
  readonly _type?: T;
}

function field<T>(
  kind: EnvKind,
  parse: (raw: string) => T,
  required = true,
  values?: readonly string[],
  extra?: { items?: EnvKind; separator?: string; group?: string },
): EnvField<T> {
  return {
    __usai: "env-field",
    kind,
    required,
    parse,
    ...(values === undefined ? {} : { values }),
    ...(extra?.items === undefined ? {} : { items: extra.items }),
    ...(extra?.separator === undefined ? {} : { separator: extra.separator }),
    ...(extra?.group === undefined ? {} : { group: extra.group }),
  };
}

/** The application's environment contract (`defineApp({ env })`).
 *
 * @category Environment
 */
export interface EnvDeclaration<S extends Record<string, EnvField<unknown>>> {
  readonly __usai: "env";
  readonly fields: S;
}

/** The fields of either an `env({...})` declaration or a bare field map,
 * so {@link EnvValues} takes whichever the caller has. Everything written
 * about the environment — the guide, the context's own doc comment —
 * said `EnvValues<typeof spec>`, and `spec` is the declaration, whose
 * fields sit one level down; only `EnvValues<(typeof spec)["fields"]>`
 * compiled, and nothing said so. */
type EnvFieldsOf<S> = S extends EnvDeclaration<infer F> ? F : S;

/** The typed values of a declaration: `EnvValues<typeof spec>`, where
 * `spec` is what `env({...})` returned (a bare field map works too).
 *
 * @category Environment
 */
export type EnvValues<
  S extends EnvDeclaration<Record<string, EnvField<unknown>>> | Record<string, EnvField<unknown>>,
> = {
  readonly [K in keyof EnvFieldsOf<S>]: EnvFieldsOf<S>[K] extends EnvField<infer T> ? T : never;
};

/**
 * Declare what the application needs from its environment. Values are
 * read by the host when a revision **activates** — a missing required
 * variable or an unparsable value fails activation, never the first
 * request — and reach handlers as `ctx.env`, parsed. Variables a resource
 * names (`DATABASE_URL`, `baseUrlEnv`) are required by that resource and
 * need no declaration here. Everything else works the other way round: a
 * variable **not** declared here is `undefined` in `ctx.env` even when it is
 * set in the process environment — the contract is the whole of what a world
 * can see. A module states its own with `defineModule({ env })`, which is
 * merged into this one, so its consumers do not have to mirror it.
 * `usai run` never reads `.env`; `usai dev` does.
 *
 * Field constructors (all required unless wrapped in `env.optional`):
 *
 * | Constructor | `ctx.env.X` | Accepts |
 * |---|---|---|
 * | `env.string()` | `string` | any text |
 * | `env.url()` | `string` | a URL, kept as text |
 * | `env.secret()` | `string` | any text; never printed by `inspect` |
 * | `env.int()` | `number` | an integer |
 * | `env.bool()` | `boolean` | `true`/`1`, `false`/`0` |
 * | `env.enum([...])` | the union | one of the listed values |
 * | `env.cidr()` | `string` | an address with a prefix length (`10.0.0.0/8`) |
 * | `env.list(inner)` | `T[]` | a separated list,each item checked as `inner` |
 * | `env.optional(field)` | `T \| undefined` | absent or empty → `undefined` |
 * | `env.group(name, {…})` | each `T \| undefined` | all of them or none, checked at activation |
 *
 * @example
 * ```ts
 * export default defineApp({
 *   env: env({
 *     APP_ENV: env.enum(["dev", "prod"]),
 *     SESSION_TTL_HOURS: env.optional(env.int()),
 *     WEBHOOK_SECRET: env.secret(),
 *   }),
 * });
 * // in a handler
 * const ttl = (ctx.env.SESSION_TTL_HOURS as number | undefined) ?? 24;
 * ```
 *
 * @category Environment
 */
export function env<S extends Record<string, EnvField<unknown>>>(fields: S): EnvDeclaration<S> {
  return { __usai: "env", fields };
}

/** Any text. */
env.string = (): EnvField<string> => field("string", (raw) => raw);
/** A URL (kept as text). */
env.url = (): EnvField<string> => field("url", (raw) => raw);
/** A value that must never be printed. */
env.secret = (): EnvField<string> => field("secret", (raw) => raw);
/** An integer. */
env.int = (): EnvField<number> =>
  field("int", (raw) => {
    const n = Number(raw);
    if (!Number.isInteger(n)) throw new Error(`expected an integer, got ${JSON.stringify(raw)}`);
    return n;
  });
/** `true`/`1` or `false`/`0`. */
env.bool = (): EnvField<boolean> =>
  field("bool", (raw) => {
    if (raw === "true" || raw === "1") return true;
    if (raw === "false" || raw === "0") return false;
    throw new Error(`expected a boolean, got ${JSON.stringify(raw)}`);
  });
/** One of the listed values. */
env.enum = <const V extends readonly string[]>(values: V): EnvField<V[number]> =>
  field(
    "enum",
    (raw) => {
      if (!values.includes(raw))
        throw new Error(`expected one of ${values.join(", ")}, got ${JSON.stringify(raw)}`);
      return raw as V[number];
    },
    true,
    values,
  );
/** An address with a prefix length: `10.0.0.0/8`, `2001:db8::/32`.
 *
 * A bare address is **not** accepted. A policy that means one host should
 * say `/32`, because the difference between `10.0.0.0` and `10.0.0.0/8` is
 * the whole of what the policy does. */
env.cidr = (): EnvField<string> =>
  field("cidr", (raw) => {
    const [address, prefix, ...rest] = raw.split("/");
    if (address === undefined || prefix === undefined || rest.length > 0)
      throw new Error(
        `expected an address with a prefix length, like 10.0.0.0/8, got ${JSON.stringify(raw)}`,
      );
    return raw;
  });
/** A separated list, each item checked as `inner`.
 *
 * The item kind is in the manifest, so the **host** validates every item at
 * activation and a malformed entry stops the deployment — a parser that only
 * runs inside a world would surface it as a failing request instead, which
 * is what declaring the variable is meant to prevent.
 *
 * ```ts
 * env({ PROTECTED_PREFIXES: env.list(env.cidr()) })   // "10.0.0.0/8,192.168.0.0/16"
 * ```
 *
 * Items are trimmed and an empty item is an error, so a trailing separator
 * is caught rather than silently dropped. */
env.list = <T>(inner: EnvField<T>, options?: { separator?: string }): EnvField<T[]> => {
  const separator = options?.separator ?? ",";
  return field(
    "list",
    (raw) =>
      raw.split(separator).map((item, i) => {
        const value = item.trim();
        if (value === "") throw new Error(`item ${i + 1} is empty`);
        try {
          return inner.parse(value);
        } catch (e) {
          throw new Error(`item ${i + 1} (${JSON.stringify(value)}): ${(e as Error).message}`);
        }
      }),
    true,
    inner.values,
    { items: inner.kind, separator },
  );
};
/** Variables that go together: **all of them or none**.
 *
 * An integration usually needs several values at once — a tenant, a client
 * id, a secret, a redirect — and three out of four is a mistake, not a
 * configuration. Each field on its own cannot say that, so the rule is
 * declared over the group and the **host** checks it at activation: a partly
 * configured integration refuses the revision and names what is missing,
 * instead of being discovered by the first person who tries to sign in.
 *
 * Every field in the group becomes optional on its own, because "none" is a
 * valid answer: `ctx.env.X` is `T | undefined`, and if one of them is defined
 * they all are.
 *
 * ```ts
 * export const spec = env({
 *   DATABASE_URL: env.url(),
 *   ...env.group("entra", {
 *     ENTRA_TENANT_ID: env.string(),
 *     ENTRA_CLIENT_ID: env.string(),
 *     ENTRA_CLIENT_SECRET: env.secret(),
 *   }),
 * });
 * ```
 *
 * The group travels on the fields, so spreading the map into a larger one
 * keeps the rule.
 *
 * @category Environment
 */
env.group = <S extends Record<string, EnvField<unknown>>>(
  name: string,
  fields: S,
): { [K in keyof S]: S[K] extends EnvField<infer T> ? EnvField<T | undefined> : never } => {
  if (!name.trim()) throw new Error("env.group needs a name");
  const out: Record<string, EnvField<unknown>> = {};
  for (const [key, inner] of Object.entries(fields)) {
    out[key] = field(inner.kind, inner.parse, false, inner.values, {
      ...(inner.items === undefined ? {} : { items: inner.items }),
      ...(inner.separator === undefined ? {} : { separator: inner.separator }),
      group: name,
    });
  }
  return out as {
    [K in keyof S]: S[K] extends EnvField<infer T> ? EnvField<T | undefined> : never;
  };
};

/** Make a field optional: absent or empty gives `undefined`. */
env.optional = <T>(inner: EnvField<T>): EnvField<T | undefined> =>
  inner.values === undefined
    ? { __usai: "env-field", kind: inner.kind, required: false, parse: inner.parse }
    : {
        __usai: "env-field",
        kind: inner.kind,
        required: false,
        values: inner.values,
        parse: inner.parse,
      };

/** Resolve declared values from a raw map (what the host does at
 * activation). Throws on the first violation, naming the variable.
 *
 * @category Environment
 */
export function resolveEnv<S extends Record<string, EnvField<unknown>>>(
  decl: EnvDeclaration<S>,
  raw: Record<string, string | undefined>,
): EnvValues<S> {
  const out: Record<string, unknown> = {};
  for (const [name, f] of Object.entries(decl.fields)) {
    const value = raw[name];
    if (value === undefined || value === "") {
      if (f.required) throw new Error(`missing required environment variable ${name}`);
      out[name] = undefined;
      continue;
    }
    out[name] = f.parse(value);
  }
  return out as EnvValues<S>;
}
