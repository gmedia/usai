import { postgres } from "usai";
export const db = postgres("main", { pool: { max: 2 } });
