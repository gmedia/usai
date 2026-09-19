import { command, cron, defineModule, errors, http, publishes, type PostgresHandle, type SqlExecutor } from "@sakaladev/usai";
import { z } from "zod";
import { db } from "../resources.ts";
import { session } from "../auth/module.ts";

const sql = (ctx: { resources: Record<string, unknown> }) => ctx.resources["main"] as PostgresHandle;

const Item = z.object({ description: z.string().min(1).max(200), quantity: z.number().int().min(1).max(10_000), unitCents: z.number().int().min(0).max(100_000_000) });
const NewInvoice = z.object({
  customer: z.string().min(1).max(200),
  currency: z.enum(["USD", "EUR", "IDR"]),
  dueDate: z.string().regex(/^\d{4}-\d{2}-\d{2}$/, "YYYY-MM-DD"),
  items: z.array(Item).min(1).max(100),
});
const Status = z.enum(["draft", "issued", "paid", "overdue", "void"]);
const Invoice = z.object({
  id: z.string(), number: z.number().int(), customer: z.string(), currency: z.string(), status: Status,
  totalCents: z.number().int().min(0), dueDate: z.string().date(), issuedAt: z.string().datetime({ offset: true }).nullable(), paidAt: z.string().datetime({ offset: true }).nullable(), createdAt: z.string().datetime({ offset: true }),
});
const InvoiceWithItems = Invoice.extend({ items: z.array(Item.extend({ id: z.number().int() })) });
const Page = z.object({ items: z.array(Invoice), nextCursor: z.string().nullable() });
const ListQuery = z.object({ status: Status.optional(), limit: z.coerce.number().int().min(1).max(100).default(20), cursor: z.string().optional() });
const Id = z.object({ id: z.string().uuid() });
type InvoiceRow = z.infer<typeof Invoice>;

const columns = `id, number, customer, currency, status, total_cents::int as "totalCents", due_date::text as "dueDate", issued_at as "issuedAt", paid_at as "paidAt", created_at as "createdAt"`;

/** The events tenants can subscribe to; delivered by the webhooks module. */
export type InvoiceEvent = "invoice.issued" | "invoice.paid" | "invoice.overdue";

async function publish(ctx: { queue: { publish(topic: string, message: unknown): Promise<unknown> } }, tenantId: string, invoiceId: string, event: InvoiceEvent) {
  await ctx.queue.publish("webhook.deliver", { tenantId, invoiceId, event });
}

export const create = http.post("/invoices", { summary: "Create a draft invoice", description: "The invoice starts as a draft; issue it to make it payable and notify the tenant's webhook.", auth: session, body: NewInvoice, response: { 201: InvoiceWithItems }, resources: [db] }, async (ctx) => {
  const total = ctx.body.items.reduce((sum, i) => sum + i.quantity * i.unitCents, 0);
  // Number, header and items commit together or not at all.
  const id = await sql(ctx).transaction(async (tx) => {
    const seq = await tx.one<{ n: number }>(`update tenants set invoice_seq = invoice_seq + 1 where id = $1 returning invoice_seq as n`, [ctx.auth.tenantId]);
    const row = await tx.one<{ id: string }>(
      `insert into invoices (tenant_id, number, customer, currency, total_cents, due_date) values ($1, $2, $3, $4, $5, $6::date) returning id`,
      [ctx.auth.tenantId, seq!.n, ctx.body.customer, ctx.body.currency, total, ctx.body.dueDate],
    );
    for (const item of ctx.body.items) {
      await tx.execute(`insert into invoice_items (invoice_id, description, quantity, unit_cents) values ($1, $2, $3, $4)`, [row!.id, item.description, item.quantity, item.unitCents]);
    }
    return row!.id;
  });
  return http.created(await load(sql(ctx), ctx.auth.tenantId, id));
});

async function load(db: SqlExecutor, tenantId: string, id: string) {
  const invoice = await db.one<InvoiceRow>(`select ${columns} from invoices where tenant_id = $1 and id = $2`, [tenantId, id]);
  if (!invoice) throw errors.notFound(`invoice ${id} does not exist`);
  const items = await db.query<{ id: number; description: string; quantity: number; unitCents: number }>(`select id::int as id, description, quantity, unit_cents::int as "unitCents" from invoice_items where invoice_id = $1 order by id`, [id]);
  return { ...invoice, items };
}

export const get = http.get("/invoices/:id", { summary: "One invoice with its line items", auth: session, params: Id, response: { 200: InvoiceWithItems }, errors: [{ code: "not_found", status: 404 }], resources: [db] }, async (ctx) =>
  load(sql(ctx), ctx.auth.tenantId, ctx.params.id),
);

// Keyset pagination on (created_at, id): stable under inserts, no OFFSET.
export const list = http.get("/invoices", { summary: "List the tenant's invoices", description: "Newest first, cursor-paginated; filter by status.", auth: session, query: ListQuery, response: { 200: Page }, resources: [db] }, async (ctx) => {
  const params: unknown[] = [ctx.auth.tenantId, ctx.query.limit + 1];
  let where = `tenant_id = $1`;
  if (ctx.query.status) { params.push(ctx.query.status); where += ` and status = $${params.length}`; }
  if (ctx.query.cursor) {
    const [createdAt, id] = atob(ctx.query.cursor).split("|");
    if (!createdAt || !id) throw errors.badRequest("malformed cursor");
    params.push(createdAt, id);
    where += ` and (created_at, id) < ($${params.length - 1}::timestamptz, $${params.length}::uuid)`;
  }
  const rows = await sql(ctx).query<InvoiceRow>(`select ${columns} from invoices where ${where} order by created_at desc, id desc limit $2`, params as never);
  const items = rows.slice(0, ctx.query.limit);
  const last = items[items.length - 1];
  return { items, nextCursor: rows.length > ctx.query.limit && last ? btoa(`${last.createdAt}|${last.id}`) : null };
});

async function transition(ctx: { resources: Record<string, unknown>; auth: { tenantId: string }; params: { id: string } }, from: string[], to: string, stamp: string) {
  const row = await sql(ctx).one<InvoiceRow>(
    `update invoices set status = $3::invoice_status, ${stamp} = now() where tenant_id = $1 and id = $2 and status = any($4::invoice_status[]) returning ${columns}`,
    [ctx.auth.tenantId, ctx.params.id, to, from],
  );
  if (!row) {
    const exists = await sql(ctx).one<{ status: string }>(`select status from invoices where tenant_id = $1 and id = $2`, [ctx.auth.tenantId, ctx.params.id]);
    if (!exists) throw errors.notFound(`invoice ${ctx.params.id} does not exist`);
    throw errors.conflict(`invoice is ${exists.status}; cannot make it ${to}`, { status: exists.status });
  }
  return row;
}

// `publishes(...)` records the hand-off to the queue for `usai graph` and the API docs.
export const issue = publishes(http.post("/invoices/:id/issue", { summary: "Issue a draft", description: "Draft → issued. Publishes invoice.issued to the tenant's webhook.", auth: session, params: Id, response: { 200: Invoice }, errors: [{ code: "not_found", status: 404 }, { code: "conflict", status: 409 }], resources: [db] }, async (ctx) => {
  const row = await transition(ctx, ["draft"], "issued", "issued_at");
  await publish(ctx, ctx.auth.tenantId, row.id, "invoice.issued");
  return row;
}), "webhook.deliver");

export const pay = publishes(http.post("/invoices/:id/pay", { summary: "Mark an invoice paid", description: "Issued or overdue → paid. Publishes invoice.paid.", auth: session, params: Id, response: { 200: Invoice }, errors: [{ code: "not_found", status: 404 }, { code: "conflict", status: 409 }], resources: [db] }, async (ctx) => {
  const row = await transition(ctx, ["issued", "overdue"], "paid", "paid_at");
  await publish(ctx, ctx.auth.tenantId, row.id, "invoice.paid");
  return row;
}), "webhook.deliver");

export const remove = http.delete("/invoices/:id", { summary: "Void a draft", auth: session, params: Id, response: { 204: z.null() }, errors: [{ code: "not_found", status: 404 }, { code: "conflict", status: 409 }], resources: [db] }, async (ctx) => {
  const gone = await sql(ctx).execute(`delete from invoices where tenant_id = $1 and id = $2 and status = 'draft'`, [ctx.auth.tenantId, ctx.params.id]);
  if (gone === 0) {
    const exists = await sql(ctx).one(`select 1 from invoices where tenant_id = $1 and id = $2`, [ctx.auth.tenantId, ctx.params.id]);
    if (!exists) throw errors.notFound(`invoice ${ctx.params.id} does not exist`);
    throw errors.conflict("only drafts can be deleted; void it instead");
  }
  return http.noContent();
});

// Once a day, every tenant's issued invoices past due become overdue and
// their webhooks fire. `usai cron run mark-overdue` runs it now.
export const markOverdue = publishes(cron("mark-overdue", { schedule: "15 0 * * *", overlap: "skip", timeout: "5m", resources: [db] }, async (ctx) => {
  const rows = await sql(ctx).query<{ id: string; tenantId: string }>(
    `update invoices set status = 'overdue' where status = 'issued' and due_date < current_date returning id, tenant_id as "tenantId"`,
  );
  for (const row of rows) await publish(ctx, row.tenantId, row.id, "invoice.overdue");
  return { overdue: rows.length };
}), "webhook.deliver");

// `usai app invoices:stats [tenant-slug]`
export const stats = command("invoices:stats", { resources: [db] }, async (ctx) => {
  const slug = ctx.args[0];
  const rows = await sql(ctx).query<{ tenant: string; status: string; count: number; totalCents: number }>(
    `select t.slug as tenant, i.status::text as status, count(*)::int as count, coalesce(sum(i.total_cents), 0)::int as "totalCents"
       from invoices i join tenants t on t.id = i.tenant_id
      where ($1::text is null or t.slug = $1)
      group by t.slug, i.status order by t.slug, i.status`,
    [slug ?? null],
  );
  return { rows };
});

export const invoices = defineModule({
  name: "invoices",
  workloads: [create, get, list, issue, pay, remove, markOverdue, stats],
  resources: [db],
  migrations: "./src/invoices/migrations/*.sql",
  seeders: "./src/invoices/seeders/*.ts",
});
