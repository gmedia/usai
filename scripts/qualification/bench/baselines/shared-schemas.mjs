// The contracts every JavaScript comparator validates with — the same Zod
// schemas the Usai app declares (scripts/qualification/bench/app/src/app.ts),
// applied to input AND output, so nobody is compared against `return pool.query()`.
import { z } from "zod";

export const Name = z.object({ name: z.string().min(1).max(40) });
export const Hello = z.object({ hello: z.string() });
export const Item = z.object({
  sku: z.string().min(1).max(32),
  quantity: z.number().int().min(1).max(1000),
  unitCents: z.number().int().min(0).max(10_000_000),
});
export const Quote = z.object({
  customer: z.string().min(1).max(200),
  email: z.string().email(),
  currency: z.enum(["USD", "EUR", "IDR"]),
  country: z.string().length(2),
  couponCode: z.string().max(20).optional(),
  notes: z.string().max(500).optional(),
  priority: z.enum(["low", "normal", "high"]).default("normal"),
  gift: z.boolean().default(false),
  requestedDate: z.string().regex(/^\d{4}-\d{2}-\d{2}$/),
  reference: z.string().uuid(),
  tags: z.array(z.string().max(16)).max(10),
  items: z.array(Item).min(1).max(50),
});
export const Quoted = z.object({
  reference: z.string(),
  currency: z.string(),
  subtotalCents: z.number().int(),
  taxCents: z.number().int(),
  totalCents: z.number().int(),
  lines: z.number().int(),
  priority: z.string(),
});
export const Id = z.object({ id: z.coerce.number().int().min(1).max(2_147_483_647) });
export const User = z.object({ id: z.number().int(), name: z.string(), email: z.string() });
export const NewUser = z.object({ name: z.string().min(1).max(200), email: z.string().email() });
export const Paid = z.object({
  orderId: z.number().int(),
  paymentId: z.number().int(),
  amountCents: z.number().int(),
  paid: z.boolean(),
});
export const Counter = z.object({ count: z.number().int() });
export const SlowQuery = z.object({ ms: z.coerce.number().int().min(0).max(30_000).default(1000) });
export const Slept = z.object({ slept: z.number().int() });

/** The quote computation, identical everywhere. */
export function quote(body) {
  const subtotal = body.items.reduce((sum, item) => sum + item.quantity * item.unitCents, 0);
  const rate = body.country === "ID" ? 11 : body.country === "DE" ? 19 : 0;
  const tax = Math.round((subtotal * rate) / 100);
  return {
    reference: body.reference,
    currency: body.currency,
    subtotalCents: subtotal,
    taxCents: tax,
    totalCents: subtotal + tax,
    lines: body.items.length,
    priority: body.priority,
  };
}

/** The error envelope every comparator answers with (Usai's shape). */
export const err = (code, message, details) => ({
  error: details === undefined ? { code, message } : { code, message, details },
});
export const issues = (zodError) =>
  zodError.issues.map((i) => ({ message: i.message, path: "/" + i.path.join("/") }));
