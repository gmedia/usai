#!/usr/bin/env node
// The cost side: the first request after the quiet period re-faults the pages
// that were released, the next one does not. One connection, so the two are
// comparable.
const [url, bodyFile] = process.argv.slice(2);
const body = await (await import("node:fs/promises")).readFile(bodyFile, "utf8");
const init = { method: "POST", headers: { "content-type": "application/json" }, body };
for (const label of ["first-request-after-idle", "second-request", "third-request"]) {
  const t = process.hrtime.bigint();
  const r = await fetch(url, init);
  await r.arrayBuffer();
  const us = Number(process.hrtime.bigint() - t) / 1000;
  console.log(`${label}\t${us.toFixed(0)}us\t${r.status}`);
}
