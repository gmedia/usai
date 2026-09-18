import { postgres } from "usai";

// One pool for the runtime's lifetime; every operation leases a connection
// and the runtime decides reuse from terminal proof (contract C5).
export const db = postgres("main", { pool: { max: 8 } });
