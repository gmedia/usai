// Declared environment requirements (`GOAL.md` §32). Values are resolved by
// the host at activation; missing required values fail activation, not the
// first request.

export type EnvKind = "string" | "url" | "secret" | "int" | "bool" | "enum";

export interface EnvField<T> {
  readonly __usai: "env-field";
  readonly kind: EnvKind;
  readonly required: boolean;
  readonly values?: readonly string[];
  readonly parse: (raw: string) => T;
  readonly _type?: T;
}

function field<T>(kind: EnvKind, parse: (raw: string) => T, required = true, values?: readonly string[]): EnvField<T> {
  return values === undefined ? { __usai: "env-field", kind, required, parse } : { __usai: "env-field", kind, required, parse, values };
}

export interface EnvDeclaration<S extends Record<string, EnvField<unknown>>> {
  readonly __usai: "env";
  readonly fields: S;
}

export type EnvValues<S extends Record<string, EnvField<unknown>>> = {
  readonly [K in keyof S]: S[K] extends EnvField<infer T> ? T : never;
};

/** Declare what the application needs from its environment. */
export function env<S extends Record<string, EnvField<unknown>>>(fields: S): EnvDeclaration<S> {
  return { __usai: "env", fields };
}

env.string = (): EnvField<string> => field("string", (raw) => raw);
env.url = (): EnvField<string> => field("url", (raw) => raw);
env.secret = (): EnvField<string> => field("secret", (raw) => raw);
env.int = (): EnvField<number> =>
  field("int", (raw) => {
    const n = Number(raw);
    if (!Number.isInteger(n)) throw new Error(`expected an integer, got ${JSON.stringify(raw)}`);
    return n;
  });
env.bool = (): EnvField<boolean> =>
  field("bool", (raw) => {
    if (raw === "true" || raw === "1") return true;
    if (raw === "false" || raw === "0") return false;
    throw new Error(`expected a boolean, got ${JSON.stringify(raw)}`);
  });
env.enum = <const V extends readonly string[]>(values: V): EnvField<V[number]> =>
  field("enum", (raw) => {
    if (!values.includes(raw)) throw new Error(`expected one of ${values.join(", ")}, got ${JSON.stringify(raw)}`);
    return raw as V[number];
  }, true, values);
env.optional = <T>(inner: EnvField<T>): EnvField<T | undefined> =>
  inner.values === undefined
    ? { __usai: "env-field", kind: inner.kind, required: false, parse: inner.parse }
    : { __usai: "env-field", kind: inner.kind, required: false, values: inner.values, parse: inner.parse };

/** Resolve declared values from a raw map. Throws on the first violation. */
export function resolveEnv<S extends Record<string, EnvField<unknown>>>(decl: EnvDeclaration<S>, raw: Record<string, string | undefined>): EnvValues<S> {
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
