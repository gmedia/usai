import { defineModule } from "usai";
import { db } from "../resources.ts";

export const billing = defineModule({
  name: "billing",
  resources: [db],
  migrations: "./src/billing/migrations/*.sql",
});
