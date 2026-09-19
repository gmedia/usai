import { test } from "node:test";
import assert from "node:assert/strict";
import { z } from "zod";
import { structuralSample } from "./prepare.ts";

// The accepting-path warm-up parses a value the schema accepts; it must
// never derive one for a schema that could run application code on that
// path, and the one it derives must actually be accepted.
test("structural schemas get an accepted sample", () => {
  const schemas = [
    z.object({ name: z.string().min(1).max(40) }),
    z.object({ id: z.coerce.number().int().min(1).max(2_147_483_647) }),
    z.object({ email: z.string().email(), when: z.string().regex(/^\d{4}-\d{2}-\d{2}$/), ref: z.string().uuid(), kind: z.enum(["a", "b"]), n: z.number().int().min(5).max(9), flag: z.boolean().default(false), tags: z.array(z.string().max(3)).min(2), opt: z.string().optional(), nested: z.object({ x: z.literal("x") }) }),
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
