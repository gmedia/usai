# invoicing — the P4 application

Built as a *user* of `@sakaladev/usai`, to find what a real API needs and
whether the model gets in the way. Tenants, users and bearer sessions
(Argon2id passwords, random tokens, only hashes stored), invoices with line
items created in one transaction, keyset pagination, status transitions
with proper 404/409, a daily cron, a command, signed webhooks delivered over
outbound HTTP through a retrying queue, colocated migrations and a seeder,
and a test through the real runtime.

```bash
export DATABASE_URL=postgres://user:pass@localhost:5432/invoicing
usai db migrate && usai db seed demo
usai dev
curl -s -X POST localhost:3000/login -H 'content-type: application/json' \
  -d '{"tenant":"demo","email":"owner@demo.test","password":"demo-demo-demo-demo"}'
# → {"token":"…","expiresAt":"…"}; then:
curl -s localhost:3000/invoices -H "authorization: Bearer $TOKEN"
usai cron run mark-overdue          # invoice 3 becomes overdue and its webhook fires
usai app invoices:stats demo
usai test                            # needs DATABASE_URL (a throwaway database)
```

Point a tenant at a webhook sink with `PUT /tenant/webhook {url, secret}`;
each delivery carries `x-invoicing-signature: sha256=<HMAC of the body>`.
