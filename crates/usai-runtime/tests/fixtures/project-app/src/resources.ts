import { postgres } from "@sakaladev/usai";
export const db = postgres("main", { pool: { max: 2 } });
