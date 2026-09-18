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
  "object", "array", "tuple", "record", "map", "set",
  "string", "number", "int", "bigint", "boolean", "date", "symbol",
  "null", "undefined", "void", "never", "literal", "enum", "nan", "file", "template_literal",
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
  return (d.checks ?? []).some((c) => typeof (c as { _zod?: ZodInternals })?._zod?.def?.when === "function");
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
  if (ZOD_FORWARDING.has(type)) return (d.checks ?? []).length === 0 && zodFailsFast(d.innerType, depth + 1);
  if (type === "pipe") return (d.checks ?? []).length === 0 && zodFailsFast(d.in, depth + 1);
  return false;
}

function zodPrepare(root: unknown): number {
  const seen = new Set<object>();
  let touched = 0;
  const run = (zi: ZodInternals, value: unknown): void => {
    try { zi.run?.({ value, issues: [] }, { async: false }); } catch { /* the library's business */ }
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
      try { void zi[k]; } catch { /* a getter that throws is the library's business */ }
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
    for (const k of ["element", "innerType", "in", "out", "valueType", "keyType", "catchall", "left", "right"]) if (d[k]) visit(d[k]);
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
    if (typeof hook === "function") { hook(); return 1; }
    return 0;
  } catch {
    return 0;
  }
}
