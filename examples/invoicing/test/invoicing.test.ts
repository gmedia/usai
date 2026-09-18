import { test } from "node:test";
import assert from "node:assert/strict";
import { createHmac } from "node:crypto";
import { createServer, type IncomingMessage } from "node:http";
import { testApp } from "@sakaladev/usai/test";

// Needs a database: DATABASE_URL (a throwaway one — the test migrates it).
const url = process.env["DATABASE_URL"];

interface Received { event: string; signature: string; body: string }

/** A webhook endpoint that records what arrives and fails the first call,
 * so the queue's retry is exercised. */
async function sink() {
  const received: Received[] = [];
  let failures = 1;
  const server = createServer((req: IncomingMessage, res) => {
    let body = "";
    req.on("data", (c: Buffer) => { body += c.toString(); });
    req.on("end", () => {
      if (failures > 0) { failures--; res.writeHead(503).end("try later"); return; }
      received.push({ event: String(req.headers["x-invoicing-event"]), signature: String(req.headers["x-invoicing-signature"]), body });
      res.writeHead(204).end();
    });
  });
  await new Promise<void>((ok) => server.listen(0, "127.0.0.1", ok));
  const port = (server.address() as { port: number }).port;
  return { url: `http://127.0.0.1:${port}/hooks`, received, close: () => { server.closeAllConnections(); server.close(); } };
}

test("invoicing: tenants, sessions, transactional invoices, pagination, signed retried webhooks, cron, command", { skip: url ? false : "set DATABASE_URL" }, async () => {
  const root = new URL("..", import.meta.url).pathname;
  const hooks = await sink();
  // Unique per run: the test database may be shared with earlier runs.
  const run = Date.now().toString(36);
  const acme = `acme-${run}`;
  const umbrella = `umbrella-${run}`;
  let app: Awaited<ReturnType<typeof testApp>>;
  try {
    app = await testApp({ root, env: { DATABASE_URL: url!, SESSION_TTL_HOURS: "1" }, migrate: { seed: true } });
  } catch (e) {
    hooks.close();
    throw e;
  }
  try {
    // Sign up: tenant + owner in one transaction; a weak password is refused before any world exists.
    const weak = await app.http.post("/signup", { body: { tenant: acme, email: "ada@acme.test", password: "short" } });
    assert.equal(weak.status, 400);
    const signup = await app.http.post("/signup", { body: { tenant: acme, email: "ada@acme.test", password: "correct horse battery staple" } });
    assert.equal(signup.status, 201, signup.text);
    const { token } = signup.body as { token: string };
    const duplicate = await app.http.post("/signup", { body: { tenant: acme, email: "bob@acme.test", password: "another long password" } });
    assert.equal(duplicate.status, 409);
    const auth = { authorization: `Bearer ${token}` };

    assert.equal((await app.http.get("/me")).status, 401, "no token");
    assert.equal((await app.http.get("/me", { headers: { authorization: "Bearer nope" } })).status, 401, "unknown token");
    const me = await app.http.get("/me", { headers: auth });
    assert.equal(me.status, 200, me.text);
    assert.equal((me.body as { tenant: string }).tenant, acme);

    const login = await app.http.post("/login", { body: { tenant: acme, email: "ADA@acme.test", password: "correct horse battery staple" } });
    assert.equal(login.status, 200, login.text);
    const wrong = await app.http.post("/login", { body: { tenant: acme, email: "ada@acme.test", password: "not the password, sorry" } });
    assert.equal(wrong.status, 401);

    // Webhook endpoint for this tenant.
    const secret = "a-shared-secret-of-sufficient-length";
    assert.equal((await app.http.put("/tenant/webhook", { headers: auth, body: { url: hooks.url, secret } })).status, 200);

    // Invoices: created in one transaction with their items.
    const created = await app.http.post("/invoices", { headers: auth, body: { customer: "Globex", currency: "USD", dueDate: "2020-01-01", items: [{ description: "Design", quantity: 2, unitCents: 5000 }, { description: "Hosting", quantity: 1, unitCents: 1999 }] } });
    assert.equal(created.status, 201, created.text);
    const invoice = created.body as { id: string; number: number; totalCents: number; status: string; items: unknown[] };
    assert.equal(invoice.number, 1);
    assert.equal(invoice.totalCents, 11999);
    assert.equal(invoice.items.length, 2);
    const invalid = await app.http.post("/invoices", { headers: auth, body: { customer: "", currency: "GBP", dueDate: "soon", items: [] } });
    assert.equal(invalid.status, 400);
    for (let i = 0; i < 4; i++) {
      const r = await app.http.post("/invoices", { headers: auth, body: { customer: `C${i}`, currency: "EUR", dueDate: "2030-01-01", items: [{ description: "x", quantity: 1, unitCents: 100 * (i + 1) }] } });
      assert.equal(r.status, 201, r.text);
    }

    // Pagination: 5 invoices, pages of 2, stable cursor.
    const page1 = await app.http.get("/invoices", { headers: auth, query: { limit: "2" } });
    assert.equal(page1.status, 200, page1.text);
    const p1 = page1.body as { items: Array<{ number: number }>; nextCursor: string | null };
    assert.deepEqual(p1.items.map((i) => i.number), [5, 4]);
    assert.ok(p1.nextCursor);
    const page2 = await app.http.get("/invoices", { headers: auth, query: { limit: "2", cursor: p1.nextCursor! } });
    const p2 = page2.body as { items: Array<{ number: number }>; nextCursor: string | null };
    assert.deepEqual(p2.items.map((i) => i.number), [3, 2]);
    const page3 = await app.http.get("/invoices", { headers: auth, query: { limit: "2", cursor: p2.nextCursor! } });
    const p3 = page3.body as { items: Array<{ number: number }>; nextCursor: string | null };
    assert.deepEqual(p3.items.map((i) => i.number), [1]);
    assert.equal(p3.nextCursor, null);

    // Transitions: 409 with the current status; 404 for another tenant's invoice.
    const issued = await app.http.post(`/invoices/${invoice.id}/issue`, { headers: auth });
    assert.equal(issued.status, 200, issued.text);
    assert.equal((issued.body as { status: string }).status, "issued");
    const again = await app.http.post(`/invoices/${invoice.id}/issue`, { headers: auth });
    assert.equal(again.status, 409);
    assert.equal((again.body as { error: { details: { status: string } } }).error.details.status, "issued");
    const deleteIssued = await app.http.delete(`/invoices/${invoice.id}`, { headers: auth });
    assert.equal(deleteIssued.status, 409);

    const other = await app.http.post("/signup", { body: { tenant: umbrella, email: "eve@umbrella.test", password: "a completely different secret" } });
    const otherAuth = { authorization: `Bearer ${(other.body as { token: string }).token}` };
    assert.equal((await app.http.get(`/invoices/${invoice.id}`, { headers: otherAuth })).status, 404, "tenants do not see each other");
    assert.equal(((await app.http.get("/invoices", { headers: otherAuth })).body as { items: unknown[] }).items.length, 0);

    // The issued event reaches the webhook: first attempt 503, second delivered, signature valid.
    const delivered = await waitFor(() => hooks.received.length >= 1, 20_000);
    assert.ok(delivered, "webhook not delivered in time");
    const hit = hooks.received[0]!;
    assert.equal(hit.event, "invoice.issued");
    const expected = `sha256=${createHmac("sha256", secret).update(hit.body).digest("hex")}`;
    assert.equal(hit.signature, expected, "HMAC signature");
    const payload = JSON.parse(hit.body) as { attempt: number; invoice: { number: number; status: string } };
    assert.equal(payload.attempt, 2, "the second attempt is the one that landed");
    assert.equal(payload.invoice.number, 1);

    // Cron: the due date is in the past → overdue, and another webhook.
    const overdue = await app.cron("mark-overdue").run<{ overdue: number }>();
    assert.equal(overdue.ok, true, JSON.stringify(overdue.error));
    assert.ok(overdue.value.overdue >= 1, "ours (plus the seeded demo tenant's on a fresh database; no webhook there)");
    assert.ok(await waitFor(() => hooks.received.some((r) => r.event === "invoice.overdue"), 20_000), "overdue webhook");

    // Pay an overdue invoice; the command reports per tenant and status.
    const paid = await app.http.post(`/invoices/${invoice.id}/pay`, { headers: auth });
    assert.equal(paid.status, 200, paid.text);
    const stats = await app.command("invoices:stats").run<{ rows: Array<{ tenant: string; status: string; count: number }> }>([acme]);
    assert.equal(stats.ok, true, JSON.stringify(stats.error));
    assert.deepEqual(Object.fromEntries(stats.value.rows.map((r) => [r.status, r.count])), { draft: 4, paid: 1 });

    // Seeded demo tenant can log in.
    const demo = await app.http.post("/login", { body: { tenant: "demo", email: "owner@demo.test", password: "demo-demo-demo-demo" } });
    assert.equal(demo.status, 200, demo.text);

    // Logout invalidates the session.
    assert.equal((await app.http.post("/logout", { headers: auth })).status, 204);
    assert.equal((await app.http.get("/me", { headers: auth })).status, 401);
  } finally {
    await app.close();
    hooks.close();
  }
});

async function waitFor(cond: () => boolean, timeoutMs: number): Promise<boolean> {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (cond()) return true;
    await new Promise((r) => setTimeout(r, 100));
  }
  return cond();
}
