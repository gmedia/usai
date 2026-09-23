// The threat-model verification campaign (`scripts/qualification/threat/run.sh`).
//
// One route per claim in `docs/THREAT-MODEL.md` that can only be checked from
// outside the process: the document says what the runtime enforces, and this
// application gives each claim something to push against. Nothing here is an
// attack — every probe asserts the *documented* outcome, and a deviation is a
// finding against the document or the runtime.
import { defineApp, env, errors, http, socket, task } from "@sakaladev/usai";
import { z } from "zod";

const config = env({
  // A secret the campaign checks never reaches the manifest or a log line.
  THREAT_SECRET: env.string({ required: false, default: "s3cr3t-canary-value" }),
});

// --- "Malformed / invalid input reaching application code" --------------------
export const contract = http.post(
  "/contract",
  {
    body: z.object({ name: z.string().min(1).max(20), url: z.url() }),
    response: z.object({ ok: z.boolean() }),
  },
  async () => ({ ok: true }),
);

// --- "Oversized request bodies" ----------------------------------------------
// A raw route takes any bytes, so a 413 here is the bound, not a contract.
export const upload = http.raw("/upload", { method: "POST" }, async (ctx) => {
  const body = await ctx.request.bytes();
  return { status: 200, json: { bytes: body.length } };
});

// --- "Internal detail leaking in error responses" -----------------------------
export const boom = http.get("/boom", {}, async () => {
  throw new Error(`the canary is ${config.THREAT_SECRET} and this text is internal`);
});

// --- "Slow / hung handlers": both halves, awaiting and synchronous ------------
export const slow = http.get("/slow", { timeout: "300ms" }, async (ctx) => {
  await ctx.sleep("30s");
  return { ok: true };
});
export const busy = http.get("/busy", { timeout: "300ms" }, async () => {
  // Synchronous work that never yields: the CPU-slice watchdog owns this one.
  const until = Date.now() + 30_000;
  let n = 0;
  while (Date.now() < until) n = (n + 1) % 1_000_003;
  return { n };
});

// --- "Memory exhaustion by one world" ----------------------------------------
// Allocates past the per-world linear-memory bound: the world faults and the
// runtime keeps serving (the next request proves it).
export const hog = http.get("/hog", {}, async () => {
  const blocks: Uint8Array[] = [];
  for (let i = 0; i < 4096; i++) blocks.push(new Uint8Array(1024 * 1024).fill(i % 255));
  return { blocks: blocks.length };
});

// --- "Logs carry what the application says, not what the client sent" ---------
// Echoes nothing; the campaign sends a canary in the body, the query, a header
// and a cookie, then greps the log for it.
export const echo = http.post(
  "/echo",
  { body: z.object({ note: z.string().max(200) }), response: z.object({ length: z.number() }) },
  async (ctx) => ({ length: ctx.body.note.length }),
);

// --- "Leaked async work extending a request's lifetime" -----------------------
export const detach = http.get("/detach", {}, async (ctx) => {
  void ctx.sleep("5s");
  return { returned: true };
});

// --- The WebSocket the idle timeout closes ------------------------------------
export const chat = socket(
  "/socket",
  { incoming: z.object({ say: z.string().max(100) }), outgoing: z.object({ heard: z.string() }) },
  async (ctx, link) => {
    link.onMessage(async (message) => {
      await link.send({ heard: message.say });
    });
  },
);

// A task nobody declares a dispatch to: the campaign checks the WARN, not a refusal.
export const record = task("record", { input: z.object({ n: z.number() }) }, async (ctx) => ({
  n: ctx.input.n,
}));

export const handOff = http.get("/hand-off", {}, async (ctx) => {
  const handle = await ctx.tasks.dispatch(record, { n: 1 });
  return { dispatched: handle.id };
});

// --- A route that answers, so "the runtime continues" has a witness -----------
export const alive = http.get("/alive", {}, async () => ({ alive: true }));

// --- A declared error, to check the envelope's shape --------------------------
export const forbidden = http.get(
  "/forbidden",
  { errors: [{ code: "forbidden", status: 403 }] },
  async () => {
    throw errors.custom("forbidden", 403, "no");
  },
);

export default defineApp({
  name: "threat",
  description: "Verification fixture for docs/THREAT-MODEL.md.",
  env: config,
  workloads: [contract, upload, boom, slow, busy, hog, echo, detach, chat, record, handOff, alive, forbidden],
});
