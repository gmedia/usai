#!/usr/bin/env node
// N keep-alive clients hitting one endpoint for S seconds. Enough concurrency
// to touch N pooling slots, which is what the plateau is made of; not a
// throughput measurement. A curl-per-request loop cannot do this: process
// spawn dominates and in-flight concurrency never reaches N.
const [url, clientsArg, secondsArg, bodyFile] = process.argv.slice(2);
const clients = Number(clientsArg ?? 32);
const end = Date.now() + Number(secondsArg ?? 10) * 1000;
const body = bodyFile ? await (await import("node:fs/promises")).readFile(bodyFile, "utf8") : null;
const init = body
  ? { method: "POST", headers: { "content-type": "application/json" }, body }
  : undefined;
let ok = 0;
let bad = 0;
const codes = new Map();
await Promise.all(
  Array.from({ length: clients }, async () => {
    while (Date.now() < end) {
      try {
        const r = await fetch(url, init);
        await r.arrayBuffer();
        codes.set(r.status, (codes.get(r.status) ?? 0) + 1);
        if (r.ok) ok += 1;
        else bad += 1;
      } catch (e) {
        bad += 1;
        codes.set(String(e.cause?.code ?? e.name), (codes.get(String(e.cause?.code ?? e.name)) ?? 0) + 1);
      }
    }
  }),
);
console.log(
  `burst clients=${clients} ok=${ok} bad=${bad} ` +
    [...codes].map(([k, v]) => `${k}=${v}`).join(" "),
);
