import { httpClient, postgres } from "@sakaladev/usai";

// One pool for the runtime's lifetime; every statement leases a connection,
// a transaction pins one (contract C5).
export const db = postgres("main", { pool: { max: 8 } });

// Tenant webhooks go to tenant-chosen URLs, so this client has no baseUrl:
// it may call any http(s) destination, bounded and owned per request.
// Tenants configure their own webhook URL, so this client cannot name its
// destination — which is exactly the shape that makes the destination
// attacker-influenced. The runtime refuses such a client into the host's own
// network (loopback, private ranges, `169.254.169.254`); this example opts
// out **because its test points the sink at 127.0.0.1**, and a real
// deployment must not: put an egress proxy in front, point `baseUrlEnv` at
// it, and let the proxy decide what the internet is (`docs/GUIDE.md` →
// Multi-tenancy, `docs/THREAT-MODEL.md`).
export const webhooks = httpClient("webhooks", {
  timeoutMs: 5000,
  maxConcurrent: 8,
  allowPrivateNetwork: true,
});
