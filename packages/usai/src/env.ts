// Declared environment requirements (`GOAL.md` §32). Values are resolved by
// the host at activation; missing required values fail activation, not the
// first request.

/** The kinds an environment field can have; `secret` is never printed by `inspect`.
 *
 * @category Environment
 */
export type EnvKind = "string" | "url" | "secret" | "int" | "bool" | "enum";

/** One declared variable: kind, whether it is required, and its parser.
 *
 * @category Environment
 */
export interface EnvField<T> {
  readonly __usai: "env-field";
  readonly kind: EnvKind;
  readonly required: boolean;
  readonly values?: readonly string[];
  readonly parse: (raw: string) => T;
  readonly _type?: T;
}

function field<T>(
  kind: EnvKind,
  parse: (raw: string) => T,
  required = true,
  values?: readonly string[],
): EnvField<T> {
  return values === undefined
    ? { __usai: "env-field", kind, required, parse }
    : { __usai: "env-field", kind, required, parse, values };
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
 * need no declaration here. `usai run` never reads `.env`; `usai dev` does.
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
 * | `env.optional(field)` | `T \| undefined` | absent or empty → `undefined` |
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
