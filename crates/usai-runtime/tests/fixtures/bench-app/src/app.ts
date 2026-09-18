// Workload matrix for execution-path attribution (tests/profile_matrix.rs).
// One bundle, many workloads: the bundle is held constant so each row
// isolates what the *handler* costs, not what the image contains.
import { defineApp, http, task, errors } from "@sakaladev/usai";
import { z } from "zod";
import { z as zm } from "zod/mini";

// --- pure guest work (tasks: no HTTP encoding in the way) ------------------
export const empty = task("empty", {}, async () => {});
export const constant = task("constant", {}, async () => ({ ok: true }));
export const loop = task("loop", {}, async () => {
  let acc = 0;
  for (let i = 0; i < 100_000; i++) acc = (acc + i * 7) % 1_000_003;
  return acc;
});
export const objects = task("objects", {}, async () => {
  const items: Array<{ id: number; name: string; tags: string[] }> = [];
  for (let i = 0; i < 5_000; i++) items.push({ id: i, name: `item-${i}`, tags: ["a", "b"] });
  return items.length;
});
const doc = Object.fromEntries(Array.from({ length: 200 }, (_, i) => [`k${i}`, { n: i, s: `value-${i}`, l: [i, i + 1, i + 2] }]));
export const json = task("json", {}, async () => JSON.parse(JSON.stringify(doc)).k199.n);

// --- host round trips ------------------------------------------------------
export const host1 = task("host1", {}, async (ctx) => {
  await ctx.sleep(0);
  return 1;
});
export const host8 = task("host8", {}, async (ctx) => {
  for (let i = 0; i < 8; i++) await ctx.sleep(0);
  return 8;
});

// --- HTTP endpoints by validation weight -----------------------------------
export const sdkOnly = http.get("/sdk/:name", {}, async (ctx) => ({ hello: ctx.params.name }));

const MiniParams = zm.object({ name: zm.string().check(zm.minLength(1), zm.maxLength(40)) });
const MiniGreeting = zm.object({ hello: zm.string() });
export const mini = http.get("/mini/:name", { params: MiniParams, response: MiniGreeting }, async (ctx) => ({ hello: ctx.params.name }));

const Params = z.object({ name: z.string().min(1).max(40) });
const Greeting = z.object({ hello: z.string() });
export const full = http.get("/zod/:name", { params: Params, response: Greeting }, async (ctx) => {
  if (ctx.params.name === "nobody") throw errors.notFound("nobody is not here");
  return { hello: ctx.params.name };
});

// --- validator warm-up: is the cost per parse, or per first parse? --------
const Warm = z.object({ name: z.string().min(1).max(40), n: z.number().int(), tags: z.array(z.string()) });
const warmInput = { name: "x", n: 1, tags: ["a"] };
export const zod1 = task("zod1", {}, async () => Warm.parse(warmInput).n);
export const zod10 = task("zod10", {}, async () => {
  let n = 0;
  for (let i = 0; i < 10; i++) n += Warm.parse(warmInput).n;
  return n;
});
const WarmMini = zm.object({ name: zm.string().check(zm.minLength(1), zm.maxLength(40)), n: zm.int(), tags: zm.array(zm.string()) });
export const mini1 = task("mini1", {}, async () => zm.parse(WarmMini, warmInput).n);
export const mini10 = task("mini10", {}, async () => {
  let n = 0;
  for (let i = 0; i < 10; i++) n += zm.parse(WarmMini, warmInput).n;
  return n;
});

// --- realistic CRUD without a database --------------------------------------
const Item = z.object({ id: z.number().int(), title: z.string().min(1).max(120), done: z.boolean(), tags: z.array(z.string()).max(10) });
const Create = Item.omit({ id: true });
const store = new Map<number, z.infer<typeof Item>>();
for (let i = 1; i <= 50; i++) store.set(i, { id: i, title: `todo ${i}`, done: i % 3 === 0, tags: ["home", "work"].slice(0, i % 3) });

export const crudList = http.get("/todos", { response: z.array(Item) }, async () => [...store.values()].filter((t) => !t.done).slice(0, 20));
export const crudCreate = http.post("/todos", { body: Create, response: { 201: Item } }, async (ctx) => {
  const id = store.size + 1;
  const item = { id, ...ctx.body };
  store.set(id, item);
  return http.created(item);
});

export default defineApp({
  name: "bench",
  workloads: [empty, constant, loop, objects, json, host1, host8, sdkOnly, mini, full, zod1, zod10, mini1, mini10, crudList, crudCreate],
});
