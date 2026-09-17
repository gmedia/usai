import { defineApp, http, errors } from "usai";
import { z } from "zod";

const Params = z.object({ name: z.string().min(1).max(40) });
const Greeting = z.object({ hello: z.string() });

export const hello = http.get("/hello/:name", { params: Params, response: Greeting }, async (ctx) => {
  if (ctx.params.name === "nobody") throw errors.notFound("nobody is not here");
  return { hello: ctx.params.name };
});

export default defineApp({
  name: "__NAME__",
  workloads: [hello],
});
