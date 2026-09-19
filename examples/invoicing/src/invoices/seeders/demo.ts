import { password, seeder, type PostgresHandle } from "@sakaladev/usai";
import { db } from "../../resources.ts";

// `usai db seed demo`: a tenant, a user (password: "demo-demo-demo-demo") and
// three invoices in different states. Idempotent.
export default seeder({ resources: [db] }, async (ctx) => {
  const sql = ctx.resources["main"] as PostgresHandle;
  const existing = await sql.one(`select id from tenants where slug = 'demo'`);
  if (existing) return { seeded: false };
  const hash = await password.hash("demo-demo-demo-demo");
  await sql.transaction(async (tx) => {
    const tenant = await tx.one<{ id: string }>(
      `insert into tenants (slug, name, invoice_seq) values ('demo', 'Demo Co', 3) returning id`,
    );
    await tx.execute(
      `insert into users (tenant_id, email, password_hash) values ($1, 'owner@demo.test', $2)`,
      [tenant!.id, hash],
    );
    const rows = [
      [1, "Globex", "USD", "draft", 120_00, "2027-01-31", null],
      [2, "Initech", "USD", "issued", 4_500_00, "2027-02-15", "2026-09-01T09:00:00Z"],
      [3, "Umbrella", "EUR", "issued", 89_99, "2026-01-01", "2025-12-01T09:00:00Z"],
    ] as const;
    for (const [number, customer, currency, status, total, due, issuedAt] of rows) {
      const inv = await tx.one<{ id: string }>(
        `insert into invoices (tenant_id, number, customer, currency, status, total_cents, due_date, issued_at) values ($1, $2, $3, $4, $5::invoice_status, $6, $7::date, $8::timestamptz) returning id`,
        [tenant!.id, number, customer, currency, status, total, due, issuedAt],
      );
      await tx.execute(
        `insert into invoice_items (invoice_id, description, quantity, unit_cents) values ($1, 'Services', 1, $2)`,
        [inv!.id, total],
      );
    }
  });
  return { seeded: true };
});
