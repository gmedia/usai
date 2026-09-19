// Schema preparation: an optional capability next to `validate` (required)
// and `describe` (optional). Builds the validator's lazily computed state
// at definition time so a fresh world does not rebuild it on its first
// parse. It must never run application code (transforms, refinements,
// defaults, preprocessors, custom checks); it may only touch structure.
//
// Keyed by the Standard Schema `vendor` string. Libraries without an
// adapter simply pay their first-parse cost per world, as before.
import type { AnySchema } from "../schema.ts";

/** Node types whose `run(undefined)` fails on the type check before any
 * check or callback can execute (an `invalid_type` issue aborts the check
 * chain; only checks with a user `when` predicate still run). */
const ZOD_FAIL_FAST = new Set([
  "object",
  "array",
  "tuple",
  "record",
  "map",
  "set",
  "string",
  "number",
  "int",
  "bigint",
  "boolean",
  "date",
  "symbol",
  "null",
  "undefined",
  "void",
  "never",
  "literal",
  "enum",
  "nan",
  "file",
  "template_literal",
]);

/** Wrappers that, given `undefined`, either return without touching the
 * inner schema (`optional`) or forward to it (`nullable`, `readonly`,
 * `nonoptional`); they are safe when they carry no checks of their own and
 * the inner schema is safe. Everything else (`default`/`prefault` value
 * getters, `catch`, `lazy`, `transform`, `custom`, `union` options,
 * `intersection`, `promise`, `function`) may run application code. */
const ZOD_FORWARDING = new Set(["optional", "nullable", "readonly", "nonoptional"]);

type ZodInternals = {
  def: Record<string, unknown> & { type?: string; checks?: unknown[] };
  run?: (payload: { value: unknown; issues: unknown[] }, ctx: { async: boolean }) => unknown;
  [lazy: string]: unknown;
};

const internals = (s: unknown): ZodInternals | undefined =>
  s && typeof s === "object" ? (s as { _zod?: ZodInternals })._zod : undefined;

function hasWhen(d: ZodInternals["def"]): boolean {
  return (d.checks ?? []).some(
    (c) => typeof (c as { _zod?: ZodInternals })?._zod?.def?.when === "function",
  );
}

/** True when `run({ value: undefined })` on this node provably executes no
 * application code. */
function zodFailsFast(s: unknown, depth = 0): boolean {
  const zi = internals(s);
  if (!zi || !zi.def || depth > 64) return false;
  const d = zi.def;
  const type = d.type;
  if (typeof type !== "string") return false;
  if (ZOD_FAIL_FAST.has(type)) return !hasWhen(d);
  if (ZOD_FORWARDING.has(type))
    return (d.checks ?? []).length === 0 && zodFailsFast(d.innerType, depth + 1);
  if (type === "pipe") return (d.checks ?? []).length === 0 && zodFailsFast(d.in, depth + 1);
  return false;
}

function zodPrepare(root: unknown): number {
  const seen = new Set<object>();
  let touched = 0;
  const run = (zi: ZodInternals, value: unknown): void => {
    try {
      zi.run?.({ value, issues: [] }, { async: false });
    } catch {
      /* the library's business */
    }
  };
  const visit = (s: unknown): void => {
    if (!s || typeof s !== "object" || seen.has(s)) return;
    seen.add(s);
    const zi = internals(s);
    if (!zi || !zi.def) return;
    const d = zi.def;
    touched++;
    // Lazily defined structural properties (pure computations over the def).
    for (const k of ["propValues", "values", "pattern", "optin", "optout"]) {
      try {
        void zi[k];
      } catch {
        /* a getter that throws is the library's business */
      }
    }
    if (zodFailsFast(s)) {
      // The type check fails at once on `undefined`; object/array/… nodes
      // compute their normalized shape before it, which is the first cost.
      run(zi, undefined);
    }
    const shape = d.shape as Record<string, unknown> | undefined;
    if (d.type === "object" && shape && typeof shape === "object" && !hasWhen(d)) {
      // The second cost is the object's generated fast path, built on the
      // first parse of an actual object. `{}` builds it and hands every
      // property `undefined`; allowed only when each property provably
      // fails fast (so no transform, default, refinement or preprocess runs).
      if (Object.keys(shape).every((k) => zodFailsFast(shape[k]))) run(zi, {});
    }
    if (shape && typeof shape === "object") for (const k of Object.keys(shape)) visit(shape[k]);
    for (const k of [
      "element",
      "innerType",
      "in",
      "out",
      "valueType",
      "keyType",
      "catchall",
      "left",
      "right",
    ])
      if (d[k]) visit(d[k]);
    for (const k of ["options", "items"]) {
      const list = d[k];
      if (Array.isArray(list)) for (const o of list) visit(o);
    }
  };
  visit(root);
  return touched;
}

/** Prepares one schema; returns how many nodes were touched (0 when the
 * library has no adapter). Never throws. */
export function prepareSchema(schema: AnySchema): number {
  try {
    const vendor = (schema as { "~standard"?: { vendor?: string } })["~standard"]?.vendor;
    if (vendor === "zod") return zodPrepare(schema);
    const hook = (schema as { "~usai"?: { prepare?: () => unknown } })["~usai"]?.prepare;
    if (typeof hook === "function") {
      hook();
      return 1;
    }
    return 0;
  } catch {
    return 0;
  }
}

// ---- the success path -----------------------------------------------------
//
// `zodPrepare` warms what a *rejected* input touches. The lazily built state
// on the *accepting* path (the first parse of a real value through a node's
// checks) is separate and was measured at 0.4–0.8 ms on the first parse of
// every fresh world for `z.string().min(1).max(40)`, and 3 ms for a 12-field
// body. Warming it means parsing a value the schema accepts, which is only
// allowed when no application code can run on that path: every node is a
// structural type from the list below and every check is one of the
// library's own (no `refine`, `transform`, `preprocess`, `superRefine`,
// `overwrite`, `custom`, `lazy`). `default`/`prefault`/`catch` are allowed
// because the sample always supplies the value, so their getters never run.

const STRUCTURAL = new Set([
  "object",
  "array",
  "tuple",
  "record",
  "string",
  "number",
  "int",
  "bigint",
  "boolean",
  "date",
  "literal",
  "enum",
  "null",
  "undefined",
  "any",
  "unknown",
  "nan",
  "optional",
  "nullable",
  "readonly",
  "nonoptional",
  "default",
  "prefault",
  "catch",
  "union",
  "pipe",
]);
const LIBRARY_CHECKS = new Set([
  "min_length",
  "max_length",
  "length_equals",
  "greater_than",
  "less_than",
  "multiple_of",
  "string_format",
  "number_format",
  "bigint_format",
  "min_size",
  "max_size",
  "size_equals",
]);
const FORMAT_SAMPLES: Record<string, string> = {
  email: "sample@example.com",
  url: "https://example.com/",
  uri: "https://example.com/",
  uuid: "6f1a2b3c-4d5e-4f60-8a71-92b3c4d5e6f7",
  guid: "6f1a2b3c-4d5e-4f60-8a71-92b3c4d5e6f7",
  datetime: "2026-01-01T00:00:00Z",
  date: "2026-01-01",
  time: "00:00:00",
  duration: "PT1S",
  ipv4: "192.0.2.1",
  ipv6: "2001:db8::1",
  cidrv4: "192.0.2.0/24",
  cidrv6: "2001:db8::/32",
  base64: "aGVsbG8=",
  base64url: "aGVsbG8",
  e164: "+15550000000",
  emoji: "😀",
  nanoid: "V1StGXR8_Z5jdHi6B-myT",
  cuid: "cjld2cjxh0000qzrmn831i7rn",
  cuid2: "tz4a98xxat96iws9zmbrgj3a",
  ulid: "01ARZ3NDEKTSV4RRFFQ69G5FAV",
  ksuid: "0ujsszwN8NRY24YaXiTIE2VWDTS",
  xid: "9m4e2mr0ui3e8a215n4g",
  lowercase: "sample",
  uppercase: "SAMPLE",
  jwt: "eyJhbGciOiJIUzI1NiJ9.e30.ZRrHA1JJJW8opsbCGfG_HACGpVUMN_a9IV7pAx_Zmeo",
};

class NotStructural extends Error {}

/** A value the schema accepts, built from its structure alone; throws
 * `NotStructural` when the schema could run application code on the
 * accepting path or when no sample can be derived. */
function zodSample(s: unknown, depth = 0): unknown {
  const zi = internals(s);
  if (!zi || !zi.def || depth > 32) throw new NotStructural();
  const d = zi.def;
  const type = d.type;
  if (typeof type !== "string" || !STRUCTURAL.has(type)) throw new NotStructural();
  const checks = (d.checks ?? []) as Array<{ _zod?: { def?: Record<string, unknown> } }>;
  const named = checks.map((c) => c._zod?.def ?? {});
  if (
    named.some((c) => typeof c["check"] !== "string" || !LIBRARY_CHECKS.has(c["check"] as string))
  )
    throw new NotStructural();
  const num = (k: string): number | undefined => {
    const c = named.find((x) => x["check"] === k);
    return c
      ? Number(
          c[
            k === "greater_than" || k === "less_than"
              ? "value"
              : k === "multiple_of"
                ? "value"
                : "minimum" in c
                  ? "minimum"
                  : "maximum"
          ] ?? c["value"],
        )
      : undefined;
  };
  switch (type) {
    case "string": {
      const fmt = named.find((c) => c["check"] === "string_format");
      if (fmt) {
        const format = String(fmt["format"]);
        if (format === "regex") {
          // A user regex is data, not code: try a few plausible strings.
          const pattern = fmt["pattern"];
          if (!(pattern instanceof RegExp)) throw new NotStructural();
          const candidate = [
            "x",
            "2026-01-01",
            "1",
            "a",
            "sample",
            "sample@example.com",
            "2026-01-01T00:00:00Z",
            "ABC-123",
            "12345",
            "+15550000000",
            "https://example.com/",
          ].find((c) => pattern.test(c));
          if (candidate === undefined) throw new NotStructural();
          return candidate;
        }
        const sample = FORMAT_SAMPLES[format];
        if (sample === undefined) throw new NotStructural();
        return sample;
      }
      const min = named.find((c) => c["check"] === "min_length")?.["minimum"] as number | undefined;
      const exact = named.find((c) => c["check"] === "length_equals")?.["length"] as
        | number
        | undefined;
      return "x".repeat(Math.max(1, exact ?? min ?? 1));
    }
    case "number":
    case "int": {
      const gt = named.find((c) => c["check"] === "greater_than");
      const lt = named.find((c) => c["check"] === "less_than");
      const mult = named.find((c) => c["check"] === "multiple_of");
      let v = 1;
      if (gt) v = Number(gt["value"]) + (gt["inclusive"] ? 0 : 1);
      if (mult) v = Number(mult["value"]) * Math.ceil(v / Number(mult["value"]));
      if (lt && v > Number(lt["value"]) - (lt["inclusive"] ? 0 : 1)) throw new NotStructural();
      if (type === "int" || named.some((c) => c["check"] === "number_format")) v = Math.ceil(v);
      return v;
    }
    case "bigint":
      return 1n;
    case "boolean":
      return true;
    case "date":
      return new Date(0);
    case "literal":
      return (d["values"] as unknown[])?.[0];
    case "enum": {
      const e = d["entries"] as Record<string, unknown> | undefined;
      const vals = e ? Object.values(e) : [];
      if (!vals.length) throw new NotStructural();
      return vals[0];
    }
    case "null":
      return null;
    case "undefined":
      return undefined;
    case "any":
    case "unknown":
      return "sample";
    case "nan":
      return NaN;
    case "optional":
    case "nullable":
    case "readonly":
    case "nonoptional":
    case "default":
    case "prefault":
    case "catch":
      return zodSample(d["innerType"], depth + 1);
    case "pipe": {
      // `pipe` is a transform unless the out side is a plain schema
      // that accepts the in side's output (e.g. `z.coerce`); only accept
      // the identity-shaped case where both sides are structural.
      const out = internals(d["out"]);
      if (!out || (out.def.type as string) === "transform") throw new NotStructural();
      return zodSample(d["in"], depth + 1);
    }
    case "union": {
      const options = d["options"] as unknown[];
      if (!Array.isArray(options) || !options.length) throw new NotStructural();
      return zodSample(options[0], depth + 1);
    }
    case "array": {
      const min = named.find((c) => c["check"] === "min_length")?.["minimum"] as number | undefined;
      const n = Math.max(1, min ?? 1);
      const item = zodSample(d["element"], depth + 1);
      return Array.from({ length: n }, () => item);
    }
    case "tuple":
      return ((d["items"] as unknown[]) ?? []).map((t) => zodSample(t, depth + 1));
    case "record":
      return { key: zodSample(d["valueType"], depth + 1) };
    case "object": {
      const shape = d["shape"] as Record<string, unknown>;
      if (!shape || typeof shape !== "object") throw new NotStructural();
      if (d["catchall"] && (internals(d["catchall"])?.def.type as string) !== "never")
        throw new NotStructural();
      const out: Record<string, unknown> = {};
      for (const k of Object.keys(shape)) out[k] = zodSample(shape[k], depth + 1);
      return out;
    }
    default:
      throw new NotStructural();
  }
}

/** A value the schema provably accepts without running application code,
 * or `undefined` when no such value can be derived. Zod only. */
export function structuralSample(schema: AnySchema): { value: unknown } | undefined {
  try {
    const vendor = (schema as { "~standard"?: { vendor?: string } })["~standard"]?.vendor;
    if (vendor !== "zod") return undefined;
    return { value: zodSample(schema) };
  } catch {
    return undefined;
  }
}

// ---- validate once ----------------------------------------------------------
//
// The host validates every input slot that has a JSON Schema before the
// world exists (C6). The guest parsed the same value again because a
// validator may do more than accept or reject: fill defaults, coerce,
// transform, strip unknown keys. When the schema provably does none of
// that — its output *is* its input for every value it accepts — the second
// parse is work the semantics never asked for, and the guest replaces it
// with a *finalizer* for the slots the host reports as validated. The
// finalizer reproduces the two things Zod does to an accepted value beyond
// accepting it — filling `.default()`s and stripping undeclared keys —
// because both are Zod's own, not application code. Anything unproven keeps
// the double pass; correctness first.
//
// The finalizer is exact by construction, not by re-implementation: it
// walks the value along the schema's structure, runs each node's own
// library checks (`check._zod.check`, the functions Zod's parse would run —
// the whitelist guarantees none is application code) and strips the keys a
// plain `z.object` does not declare (the one structural effect Zod has on
// an accepted value; `strictObject` refused them at the host already,
// `looseObject` keeps them). If any check or type test fails — the host and
// Zod disagree on a length counted in code points vs UTF-16 units, on a
// Unicode digit in a regex — it yields `REPARSE` and the guest runs the
// full Zod parse, whose issues are then reported exactly as before.

/** Returns the value Zod's parse would return, or `REPARSE` when only the
 * full parse can decide (and produce the issues). */
export type Finalizer = (value: unknown) => unknown;
/** Sentinel: the fast path could not prove acceptance; parse for real. */
export const REPARSE: unique symbol = Symbol("usai.reparse");

class NotFinal extends Error {}

type ZodCheckFn = (payload: { value: unknown; issues: unknown[] }) => unknown;

/** The node's own checks as one function: true when all pass. */
function checksOf(d: ZodInternals["def"]): ((v: unknown) => boolean) | undefined {
  const checks = (d.checks ?? []) as Array<{
    _zod?: { def?: Record<string, unknown>; check?: ZodCheckFn };
  }>;
  if (checks.length === 0) return undefined;
  const fns: ZodCheckFn[] = [];
  for (const c of checks) {
    const kind = c._zod?.def?.["check"];
    const fn = c._zod?.check;
    if (typeof kind !== "string" || !LIBRARY_CHECKS.has(kind) || typeof fn !== "function")
      throw new NotFinal();
    fns.push(fn);
  }
  return (v) => {
    for (const fn of fns) {
      const payload = { value: v, issues: [] as unknown[] };
      fn(payload);
      if (payload.issues.length > 0) return false;
    }
    return true;
  };
}

/** A scalar node: type test plus the node's checks; the value itself is
 * the output. */
function scalar(test: (v: unknown) => boolean, d: ZodInternals["def"]): Finalizer {
  const checks = checksOf(d);
  return (v) => (test(v) && (checks === undefined || checks(v)) ? v : REPARSE);
}

const isPlainObject = (v: unknown): v is Record<string, unknown> =>
  typeof v === "object" && v !== null && !Array.isArray(v);

function zodFinalizer(s: unknown, depth = 0): Finalizer {
  const zi = internals(s);
  if (!zi || !zi.def || depth > 32) throw new NotFinal();
  const d = zi.def;
  const type = d.type;
  if (typeof type !== "string") throw new NotFinal();
  // `z.coerce.*` is the base type with a `coerce` flag: `Number(v)` of a
  // number is the number, so a value that already has the type (the host
  // coerced URL scalars against the JSON Schema) is final; anything else
  // fails the type test below and is reparsed, where Zod coerces it.
  switch (type) {
    case "string":
      return scalar((v) => typeof v === "string", d);
    case "number":
      return scalar((v) => typeof v === "number" && Number.isFinite(v), d);
    case "int":
      return scalar((v) => typeof v === "number" && Number.isInteger(v), d);
    case "boolean":
      return scalar((v) => typeof v === "boolean", d);
    case "null":
      return scalar((v) => v === null, d);
    case "literal": {
      const values = new Set((d["values"] as unknown[]) ?? []);
      return scalar((v) => values.has(v), d);
    }
    case "enum": {
      const values = new Set(Object.values((d["entries"] as Record<string, unknown>) ?? {}));
      return scalar((v) => values.has(v), d);
    }
    case "any":
    case "unknown":
      return scalar(() => true, d);
    case "optional":
    case "nullable":
    case "nonoptional": {
      if ((d.checks ?? []).length > 0) throw new NotFinal();
      const inner = zodFinalizer(d["innerType"], depth + 1);
      if (type === "optional") return (v) => (v === undefined ? v : inner(v));
      if (type === "nullable") return (v) => (v === null ? v : inner(v));
      return (v) => (v === undefined ? REPARSE : inner(v));
    }
    case "default": {
      // Zod 4: an absent value *is* the default, unparsed; a present one is
      // the inner schema's output. `defaultValue` is a getter that may call
      // the application's factory — at request time, exactly when Zod's own
      // parse would call it, never at proof time.
      if ((d.checks ?? []).length > 0) throw new NotFinal();
      const inner = zodFinalizer(d["innerType"], depth + 1);
      return (v) => (v === undefined ? (d as { defaultValue?: unknown }).defaultValue : inner(v));
    }
    case "union": {
      if ((d.checks ?? []).length > 0) throw new NotFinal();
      const options = ((d["options"] as unknown[]) ?? []).map((o) => zodFinalizer(o, depth + 1));
      if (!options.length) throw new NotFinal();
      // First accepting option wins, as in Zod.
      return (v) => {
        for (const option of options) {
          const out = option(v);
          if (out !== REPARSE) return out;
        }
        return REPARSE;
      };
    }
    case "array": {
      const item = zodFinalizer(d["element"], depth + 1);
      const checks = checksOf(d);
      return (v) => {
        if (!Array.isArray(v) || (checks !== undefined && !checks(v))) return REPARSE;
        const out = new Array<unknown>(v.length);
        for (let i = 0; i < v.length; i++) {
          const x = item(v[i]);
          if (x === REPARSE) return REPARSE;
          out[i] = x;
        }
        return out;
      };
    }
    case "tuple": {
      const items = ((d["items"] as unknown[]) ?? []).map((t) => zodFinalizer(t, depth + 1));
      const rest = d["rest"] ? zodFinalizer(d["rest"], depth + 1) : undefined;
      const checks = checksOf(d);
      return (v) => {
        if (!Array.isArray(v) || (checks !== undefined && !checks(v))) return REPARSE;
        if (rest === undefined ? v.length !== items.length : v.length < items.length)
          return REPARSE;
        const out = new Array<unknown>(v.length);
        for (let i = 0; i < v.length; i++) {
          const x = (items[i] ?? rest!)(v[i]);
          if (x === REPARSE) return REPARSE;
          out[i] = x;
        }
        return out;
      };
    }
    case "record": {
      const key = zodFinalizer(d["keyType"], depth + 1);
      const value = zodFinalizer(d["valueType"], depth + 1);
      const checks = checksOf(d);
      return (v) => {
        if (!isPlainObject(v) || (checks !== undefined && !checks(v))) return REPARSE;
        const out: Record<string, unknown> = {};
        for (const k of Object.keys(v)) {
          if (key(k) === REPARSE) return REPARSE;
          const x = value(v[k]);
          if (x === REPARSE) return REPARSE;
          out[k] = x;
        }
        return out;
      };
    }
    case "object": {
      const shape = d["shape"] as Record<string, unknown>;
      if (!isPlainObject(shape)) throw new NotFinal();
      const keys = Object.keys(shape);
      const fields = keys.map((k) => zodFinalizer(shape[k], depth + 1));
      // Zod skips an absent key only for an optional field; every other
      // field runs with `undefined` (a default fills in, anything else fails).
      const optional = keys.map(
        (k) => (internals(shape[k]) as { optin?: string } | undefined)?.optin === "optional",
      );
      const catchall = d["catchall"]
        ? (internals(d["catchall"])?.def.type as string | undefined)
        : undefined;
      // undefined: plain object, undeclared keys are stripped. never:
      // strictObject, the host refused them. unknown/any: looseObject,
      // they pass through. A typed catchall validates them: not proven.
      if (
        catchall !== undefined &&
        catchall !== "never" &&
        catchall !== "unknown" &&
        catchall !== "any"
      )
        throw new NotFinal();
      const keep = catchall === "unknown" || catchall === "any";
      const checks = checksOf(d);
      return (v) => {
        if (!isPlainObject(v) || (checks !== undefined && !checks(v))) return REPARSE;
        const out: Record<string, unknown> = {};
        for (let i = 0; i < keys.length; i++) {
          const k = keys[i]!;
          if (!(k in v)) {
            if (optional[i]) continue;
            const x = fields[i]!(undefined);
            if (x === REPARSE) return REPARSE;
            out[k] = x;
            continue;
          }
          const x = fields[i]!(v[k]);
          if (x === REPARSE) return REPARSE;
          out[k] = x;
        }
        if (keep)
          for (const k of Object.keys(v)) if (!(k in out) && !keys.includes(k)) out[k] = v[k];
        return out;
      };
    }
    default:
      // date, bigint, nan, readonly, prefault, catch, pipe, transform,
      // lazy, custom, intersection, map, set, …: the output may differ from
      // the input or application code may run.
      throw new NotFinal();
  }
}

/** The finalizer for a schema whose output is provably its (host-validated)
 * input — plus Zod's own defaults and key stripping — or `undefined` when
 * the schema may change the value or run application code (transforms,
 * preprocess, catch, prefault, readonly, dates, `.trim()`, …). Zod only. */
export function hostFinal(schema: AnySchema): Finalizer | undefined {
  try {
    const vendor = (schema as { "~standard"?: { vendor?: string } })["~standard"]?.vendor;
    if (vendor !== "zod") return undefined;
    return zodFinalizer(schema);
  } catch {
    return undefined;
  }
}
