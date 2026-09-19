// A live view of the tenant's invoices: server-sent events and a WebSocket.
// Both are connection-bound worlds — one per connection, ended by the
// client, the deadline, or a draining revision — and the second half of
// what the reliability campaign (`scripts/qualification/p6`) churns.
import { http, socket } from "@sakaladev/usai";
import { z } from "zod";
import { db } from "../resources.ts";
import { session } from "../auth/module.ts";

const Counts = z.object({
  count: z.number().int(),
  overdue: z.number().int(),
  issuedTotalCents: z.number().int(),
});
type CountsRow = z.infer<typeof Counts>;

const countsSql = `select count(*)::int as count,
                          count(*) filter (where status = 'overdue')::int as overdue,
                          coalesce(sum(total_cents) filter (where status in ('issued', 'overdue')), 0)::int as "issuedTotalCents"
                     from invoices where tenant_id = $1`;

/** `GET /invoices/live`: one `counts` event now, then one every `everyMs`
 * until the client goes away. */
export const live = http.stream(
  "/invoices/live",
  {
    summary: "Live counts as server-sent events",
    description:
      "Sends a `counts` event immediately and then every `everyMs` milliseconds until the client disconnects; the world lives as long as the connection.",
    auth: session,
    query: z.object({ everyMs: z.coerce.number().int().min(200).max(30_000).default(1000) }),
    resources: [db],
  },
  async (ctx, stream) => {
    while (!ctx.signal.aborted) {
      const row = await ctx.resources.main.one<CountsRow>(countsSql, [ctx.auth.tenantId]);
      await stream.event("counts", row ?? { count: 0, overdue: 0, issuedTotalCents: 0 });
      await ctx.sleep(ctx.query.everyMs);
    }
  },
);

const Ask = z.object({ type: z.literal("counts") });
const Answer = z.discriminatedUnion("type", [
  z.object({ type: z.literal("hello"), tenantId: z.string().uuid() }),
  Counts.extend({ type: z.literal("counts") }),
]);

/** `GET /invoices/socket` (WebSocket): says hello, then answers each
 * `{ "type": "counts" }` with the tenant's counts. */
export const feed = socket(
  "/invoices/socket",
  {
    summary: "Ask for counts over a WebSocket",
    description:
      'On open the server sends `hello`; each `{ "type": "counts" }` message is answered with the tenant\'s counts. One world per connection; `ctx.state` counts the questions.',
    auth: session,
    incoming: Ask,
    outgoing: Answer,
    resources: [db],
  },
  {
    open: async (ctx) => {
      ctx.state.asked = 0;
      await ctx.send({ type: "hello", tenantId: ctx.auth.tenantId });
    },
    message: async (ctx) => {
      ctx.state.asked = ((ctx.state.asked as number) ?? 0) + 1;
      const row = await ctx.resources.main.one<CountsRow>(countsSql, [ctx.auth.tenantId]);
      await ctx.send({ type: "counts", ...(row ?? { count: 0, overdue: 0, issuedTotalCents: 0 }) });
    },
  },
);
