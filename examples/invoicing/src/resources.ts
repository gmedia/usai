import { httpClient, postgres } from "@sakaladev/usai";

// One pool for the runtime's lifetime; every statement leases a connection,
// a transaction pins one (contract C5).
export const db = postgres("main", { pool: { max: 8 } });

// Tenant webhooks go to tenant-chosen URLs, so this client has no baseUrl:
// it may call any http(s) destination, bounded and owned per request.
export const webhooks = httpClient("webhooks", { timeoutMs: 5000, maxConcurrent: 8 });
