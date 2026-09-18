// Invoicing — the roadmap's P4 application, written as a user of the SDK:
// nothing here reaches into the runtime.
//
//   export DATABASE_URL=postgres://user:pass@localhost:5432/invoicing
//   usai db migrate --root examples/invoicing && usai db seed --root examples/invoicing
//   usai dev --root examples/invoicing
//   curl -X POST localhost:3000/signup -H 'content-type: application/json' \
//        -d '{"tenant":"acme","email":"ada@acme.test","password":"correct horse battery staple"}'
//   usai app invoices:stats --root examples/invoicing
//   usai test --root examples/invoicing
import { defineApp, env } from "@sakaladev/usai";
import { authModule } from "./auth/module.ts";
import { invoices } from "./invoices/module.ts";
import { webhookDelivery } from "./webhooks/module.ts";

export default defineApp({
  name: "invoicing",
  modules: [authModule, invoices, webhookDelivery],
  env: env({
    DATABASE_URL: env.url(),
    // Sessions live this long after the last login.
    SESSION_TTL_HOURS: env.optional(env.int()),
  }),
});
