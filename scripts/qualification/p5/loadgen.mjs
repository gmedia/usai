#!/usr/bin/env node
// Steady closed-loop load against the deployment: N clients, each doing
// list → get → (every 10th) create. Prints one JSON line per second with
// counts by outcome and latency percentiles; never stops on errors (that is
// what we are measuring). Usage: loadgen.mjs <base> <token> <clients> <seconds> [out.jsonl]
import { appendFileSync } from "node:fs";
const [base, token, clientsArg, secondsArg, out] = process.argv.slice(2);
const clients = Number(clientsArg ?? 8),
  seconds = Number(secondsArg ?? 60);
const headers = { authorization: `Bearer ${token}`, "content-type": "application/json" };
let window = { ok: 0, s4xx: 0, s5xx: 0, s503: 0, errors: 0, latencies: [] };
let stop = false;
async function one(i, n) {
  const t = performance.now();
  try {
    const kind = n % 10 === 0 ? "create" : "list";
    const res =
      kind === "create"
        ? await fetch(`${base}/invoices`, {
            method: "POST",
            headers,
            body: JSON.stringify({
              customer: `load-${i}`,
              currency: "USD",
              dueDate: "2030-01-01",
              items: [{ description: "x", quantity: 1, unitCents: 100 }],
            }),
            signal: AbortSignal.timeout(10_000),
          })
        : await fetch(`${base}/invoices?limit=5`, { headers, signal: AbortSignal.timeout(10_000) });
    await res.arrayBuffer();
    if (res.status < 300) window.ok++;
    else if (res.status === 503) window.s503++;
    else if (res.status < 500) window.s4xx++;
    else window.s5xx++;
  } catch {
    window.errors++;
  }
  window.latencies.push(performance.now() - t);
}
async function client(i) {
  let n = 0;
  while (!stop) await one(i, n++);
}
const pct = (a, p) => {
  if (!a.length) return null;
  const s = [...a].sort((x, y) => x - y);
  return +s[Math.min(s.length - 1, Math.floor(p * s.length))].toFixed(1);
};
const started = Date.now();
const tick = setInterval(() => {
  const w = window;
  window = { ok: 0, s4xx: 0, s5xx: 0, s503: 0, errors: 0, latencies: [] };
  const line = JSON.stringify({
    t: Math.round((Date.now() - started) / 1000),
    ok: w.ok,
    s4xx: w.s4xx,
    s5xx: w.s5xx,
    s503: w.s503,
    errors: w.errors,
    p50: pct(w.latencies, 0.5),
    p99: pct(w.latencies, 0.99),
  });
  console.log(line);
  if (out) appendFileSync(out, line + "\n");
}, 1000);
const runs = Array.from({ length: clients }, (_, i) => client(i));
setTimeout(() => {
  stop = true;
}, seconds * 1000);
await Promise.all(runs);
clearInterval(tick);
