import { defineModule, errors, queue } from "@sakaladev/usai";
import { z } from "zod";
import { db, webhooks } from "../resources.ts";

// Delivery is at-least-once with explicit retry (ADR-0014): five attempts,
// exponential backoff, each attempt in a fresh world. A tenant without a
// webhook URL is a no-op, not a failure.
const Message = z.object({
  tenantId: z.string().uuid(),
  invoiceId: z.string().uuid(),
  event: z.enum(["invoice.issued", "invoice.paid", "invoice.overdue"]),
});

const hex = (bytes: ArrayBuffer) =>
  Array.from(new Uint8Array(bytes), (b) => b.toString(16).padStart(2, "0")).join("");

export const deliver = queue.consume(
  "webhook.deliver",
  {
    message: Message,
    concurrency: 4,
    retry: { maxAttempts: 5, backoff: "exponential", baseMs: 500 },
    timeout: "15s",
    database: db,
    resources: [db, webhooks],
  },
  async (ctx) => {
    const tenant = await ctx.resources.main.one<{ url: string | null; secret: string | null }>(
      `select webhook_url as url, webhook_secret as secret from tenants where id = $1`,
      [ctx.message.tenantId],
    );
    if (!tenant?.url || !tenant.secret) return { skipped: "no webhook configured" };
    const invoice = await ctx.resources.main.one(
      `select id, number, customer, currency, status, total_cents::int as "totalCents", due_date::text as "dueDate" from invoices where id = $1 and tenant_id = $2`,
      [ctx.message.invoiceId, ctx.message.tenantId],
    );
    if (!invoice) return { skipped: "invoice gone" };

    const payload = JSON.stringify({
      event: ctx.message.event,
      attempt: ctx.attempt,
      deliveryId: ctx.id,
      invoice,
    });
    const key = await crypto.subtle.importKey(
      "raw",
      new TextEncoder().encode(tenant.secret),
      { name: "HMAC", hash: "SHA-256" },
      false,
      ["sign"],
    );
    const signature = hex(await crypto.subtle.sign("HMAC", key, new TextEncoder().encode(payload)));

    let status: number | null = null;
    let error: string | null = null;
    try {
      const res = await ctx.resources.webhooks.fetch(tenant.url, {
        method: "POST",
        body: payload,
        headers: {
          "content-type": "application/json",
          "x-invoicing-event": ctx.message.event,
          "x-invoicing-signature": `sha256=${signature}`,
        },
      });
      status = res.status;
      if (!res.ok) error = `endpoint answered ${res.status}`;
    } catch (e) {
      error = (e as Error).message;
    }
    await ctx.resources.main.execute(
      `insert into webhook_deliveries (tenant_id, event, invoice_id, attempt, status, error) values ($1, $2, $3, $4, $5, $6)`,
      [ctx.message.tenantId, ctx.message.event, ctx.message.invoiceId, ctx.attempt, status, error],
    );
    // Throwing asks for the next attempt; the last failure stays recorded.
    if (error)
      throw errors.custom(
        "delivery_failed",
        502,
        `${ctx.message.event} to ${tenant.url}: ${error}`,
      );
    return { delivered: status };
  },
);

export const webhookDelivery = defineModule({
  name: "webhooks",
  workloads: [deliver],
  resources: [db, webhooks],
});
