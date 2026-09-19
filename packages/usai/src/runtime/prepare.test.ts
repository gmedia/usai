import { test } from "node:test";
import assert from "node:assert/strict";
import { z } from "zod";
import { REPARSE, hostFinal, structuralSample } from "./prepare.ts";

// The accepting-path warm-up parses a value the schema accepts; it must
// never derive one for a schema that could run application code on that
// path, and the one it derives must actually be accepted.
test("structural schemas get an accepted sample", () => {
  const schemas = [
    z.object({ name: z.string().min(1).max(40) }),
    z.object({ id: z.coerce.number().int().min(1).max(2_147_483_647) }),
    z.object({
      email: z.string().email(),
      when: z.string().regex(/^\d{4}-\d{2}-\d{2}$/),
      ref: z.string().uuid(),
      kind: z.enum(["a", "b"]),
      n: z.number().int().min(5).max(9),
      flag: z.boolean().default(false),
      tags: z.array(z.string().max(3)).min(2),
      opt: z.string().optional(),
      nested: z.object({ x: z.literal("x") }),
    }),
    z.array(z.object({ q: z.number().min(0.5) })).min(1),
  ];
  for (const s of schemas) {
    const sample = structuralSample(s);
    assert.ok(sample, "a sample exists");
    assert.equal(s.safeParse(sample.value).success, true, JSON.stringify(sample.value));
  }
});

test("schemas that could run application code get no sample", () => {
  const unsafe = [
    z.object({ a: z.string().transform((s) => s.length) }),
    z.object({ a: z.string().refine((s) => s.length > 2) }),
    z.object({ a: z.preprocess((v) => v, z.string()) }),
    z.object({ a: z.string().superRefine(() => {}) }),
    z.lazy(() => z.string()),
    z.object({ a: z.string().regex(/^\p{Emoji}{7}$/u) }), // no candidate matches
  ];
  for (const s of unsafe) assert.equal(structuralSample(s as never), undefined, String(s));
});

// Validate once: for a schema the proof accepts, the finalizer applied to a
// host-accepted value must equal what Zod's own parse would return.
test("hostFinal reproduces zod's output for structural schemas", () => {
  const cases: Array<[z.ZodType, unknown]> = [
    [z.object({ name: z.string().min(1).max(40) }), { name: "x", extra: 1 }],
    [z.strictObject({ id: z.number().int() }), { id: 4 }],
    [z.looseObject({ id: z.number().int() }), { id: 4, more: "kept" }],
    [
      z.object({
        email: z.string().email(),
        kind: z.enum(["a", "b"]),
        n: z.number().int().min(5).max(9),
        tags: z.array(z.string().max(3)).min(2),
        opt: z.string().optional(),
        maybe: z.number().nullable(),
        nested: z.object({ x: z.literal("x"), deep: z.array(z.object({ k: z.string() })) }),
        pair: z.tuple([z.string(), z.number()]),
        rec: z.record(z.string(), z.object({ v: z.boolean() })),
        either: z.union([z.string(), z.number()]),
      }),
      {
        email: "a@b.co",
        kind: "a",
        n: 7,
        tags: ["ab", "cd"],
        maybe: null,
        nested: { x: "x", junk: true, deep: [{ k: "1", z: 2 }] },
        pair: ["p", 1],
        rec: { one: { v: true, w: 0 } },
        either: 3,
        undeclared: "gone",
      },
    ],
    [z.array(z.object({ q: z.number().min(0.5) })), [{ q: 1, r: 2 }]],
    // Defaults: absent is the default (unparsed, as Zod 4 does), present is the value.
    [
      z.object({
        p: z.enum(["a", "b"]).default("a"),
        f: z.string().default(() => "gen"),
        o: z.object({ x: z.string() }).default({ x: "d", extra: 1 } as never),
      }),
      {},
    ],
    [
      z.object({ p: z.enum(["a", "b"]).default("a"), f: z.string().default(() => "gen") }),
      { p: "b", f: "x" },
    ],
    // Coercion on a value that already has the type is the identity.
    [z.object({ id: z.coerce.number().int().min(1) }), { id: 42 }],
    // A union of objects: the first accepting option decides what is stripped.
    [z.union([z.object({ k: z.string() }), z.object({ j: z.string() })]), { j: "x", k: 1 }],
    [z.union([z.object({ k: z.string() }), z.object({ j: z.string() })]), { j: "x", k: "y" }],
  ];
  for (const [schema, value] of cases) {
    const finalize = hostFinal(schema);
    assert.ok(finalize, "the proof accepts the schema");
    assert.deepEqual(finalize(value), schema.parse(value), JSON.stringify(value));
  }
});

// Where the host's JSON Schema check and Zod disagree on an accepted value,
// the finalizer must not decide: it hands the value back to the full parse.
test("hostFinal reparses when zod's own checks would reject", () => {
  const cases: Array<[z.ZodType, unknown]> = [
    [z.object({ s: z.string().max(1) }), { s: "ab" }],
    [z.object({ d: z.string().date() }), { d: "٢٠٢٦-01-01" }], // Unicode digits
    [z.object({ n: z.number().int().max(9) }), { n: 10 }],
    [z.object({ n: z.number() }), { n: "10" }],
    [z.object({ a: z.string() }), {}],
    [z.object({ a: z.string() }), []],
    [z.tuple([z.string()]), ["a", "b"]],
    [z.union([z.literal("a"), z.literal("b")]), "c"],
    [z.array(z.string()).min(2), ["a"]],
    [z.object({ a: z.string().default("d"), b: z.string() }), { a: "x" }],
  ];
  for (const [schema, value] of cases) {
    const finalize = hostFinal(schema);
    assert.ok(finalize, "the proof accepts the schema");
    assert.equal(finalize(value), REPARSE, JSON.stringify(value));
    assert.equal(schema.safeParse(value).success, false, "zod rejects it too");
  }
  // A coercible value is not final (the finalizer never coerces); the full
  // parse then coerces it, as it always did.
  const coerce = z.object({ id: z.coerce.number().int() });
  assert.equal(hostFinal(coerce)!({ id: "42" }), REPARSE);
  assert.deepEqual(coerce.parse({ id: "42" }), { id: 42 });
});

test("hostFinal refuses schemas whose output may differ from the input", () => {
  const refused = [
    z.object({ a: z.string().transform((s) => s.length) }),
    z.object({ a: z.string().refine((s) => s.length > 2) }),
    z.object({ a: z.string().trim() }),
    z.object({ a: z.string().catch("c") }),
    z.object({ a: z.string().min(3).prefault("x") }),
    z.object({ a: z.date() }),
    z.object({ a: z.string() }).readonly(),
    z.object({ a: z.string() }).catchall(z.number()),
    z.lazy(() => z.string()),
    z.preprocess((v) => v, z.string()),
  ];
  for (const s of refused) assert.equal(hostFinal(s as never), undefined, String(s));
});
