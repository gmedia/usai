#!/usr/bin/env node
// Closed-loop load client for the suite: N clients, one keep-alive
// connection each, a workload class, a duration. Prints one JSON summary
// (req/s, p50/p95/p99, outcomes) and, when the server sends
// `x-usai-profile` (Usai with USAI_PROFILE=1), the averaged per-phase
// invoice. Node's fetch caps around 10–15k req/s on one core, so the suite
// uses it for c ≤ 4 and for classes that need unique payloads (D writes);
// oha does the high-concurrency sweeps.
//
//   load.mjs <base> <class A|B|C|D|E|F|counter> <clients> <seconds>
import { Agent, fetch, setGlobalDispatcher } from "undici";

const [base, cls, clientsArg, secondsArg] = process.argv.slice(2);
const clients = Number(clientsArg ?? 1),
  seconds = Number(secondsArg ?? 10);
setGlobalDispatcher(new Agent({ connections: clients, pipelining: 1, keepAliveTimeout: 60_000 }));
const ref = "6f1a2b3c-4d5e-4f60-8a71-92b3c4d5e6f7";
const quoteBody = JSON.stringify({
  customer: "Ayu",
  email: "ayu@example.com",
  currency: "IDR",
  country: "ID",
  requestedDate: "2026-10-01",
  reference: ref,
  tags: ["a", "b"],
  items: [
    { sku: "A1", quantity: 2, unitCents: 1500 },
    { sku: "B2", quantity: 1, unitCents: 990 },
    { sku: "C3", quantity: 5, unitCents: 120 },
  ],
});
const run = Date.now().toString(36);
let seq = 0;
const request = (i) => {
  switch (cls) {
    case "A":
      return ["GET", "/hello/world", undefined];
    case "B":
      return ["POST", "/orders/quote", quoteBody];
    case "C":
      return ["GET", `/users/${42 + (i % 7)}`, undefined];
    case "D":
      return [
        "POST",
        "/users",
        JSON.stringify({ name: "Load", email: `load-${run}-${++seq}@example.test` }),
      ];
    case "E":
      return ["POST", `/orders/${1 + Math.floor(Math.random() * 100_000)}/pay`, undefined];
    case "F":
      return ["GET", "/me", undefined];
    case "counter":
      return ["GET", "/counter", undefined];
    default:
      throw new Error(`unknown class ${cls}`);
  }
};
const headers = { "x-api-key": "key-5" };
const withBody = { "content-type": "application/json", "x-api-key": "key-5" };
const lat = [],
  phases = new Map();
let ok = 0,
  s4xx = 0,
  s5xx = 0,
  errors = 0,
  profiled = 0,
  stop = false;
async function client() {
  let i = 0;
  while (!stop) {
    const [method, path, body] = request(i++);
    const t = performance.now();
    try {
      const res = await fetch(base + path, {
        method,
        headers: body === undefined ? headers : withBody,
        body,
      });
      const prof = res.headers.get("x-usai-profile");
      await res.arrayBuffer();
      lat.push(performance.now() - t);
      if (res.status < 300) ok++;
      else if (res.status < 500) s4xx++;
      else s5xx++;
      if (prof) {
        profiled++;
        for (const item of prof.split(",")) {
          const [k, v] = item.split("=");
          phases.set(k, (phases.get(k) ?? 0) + Number(v));
        }
      }
    } catch {
      errors++;
      lat.push(performance.now() - t);
    }
  }
}
// Warm-up: not counted.
for (let i = 0; i < 50; i++) {
  const [m, p, b] = request(i);
  try {
    await (
      await fetch(base + p, { method: m, headers: b === undefined ? headers : withBody, body: b })
    ).arrayBuffer();
  } catch {}
}
lat.length = 0;
ok = s4xx = s5xx = errors = profiled = 0;
phases.clear();
const started = performance.now();
const runs = Array.from({ length: clients }, client);
setTimeout(() => {
  stop = true;
}, seconds * 1000);
await Promise.all(runs);
const elapsed = (performance.now() - started) / 1000;
lat.sort((a, b) => a - b);
const pct = (p) => +(lat[Math.min(lat.length - 1, Math.floor(p * lat.length))] ?? 0).toFixed(3);
const total = ok + s4xx + s5xx + errors;
const summary = {
  class: cls,
  clients,
  seconds: +elapsed.toFixed(1),
  requests: total,
  rps: +(total / elapsed).toFixed(0),
  p50: pct(0.5),
  p95: pct(0.95),
  p99: pct(0.99),
  ok,
  s4xx,
  s5xx,
  errors,
};
if (profiled)
  summary.invoice = Object.fromEntries(
    [...phases.entries()].map(([k, v]) => [k, +(v / profiled).toFixed(4)]),
  );
console.log(JSON.stringify(summary));
