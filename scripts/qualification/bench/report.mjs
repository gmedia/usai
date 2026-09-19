#!/usr/bin/env node
// Turns a suite output directory into the scoreboard tables (markdown):
// per class, one row per server × concurrency: req/s, p50/p95/p99, CPU per
// request, RSS; plus Usai's invoice per class when present.
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
const dir = process.argv[2];
const cells = readdirSync(dir).filter((f) => /\.c\d+\.json$/.test(f)).map((f) => JSON.parse(readFileSync(join(dir, f), "utf8")));
const classes = { A: "hello (runtime tax)", B: "contract-heavy quote", C: "DB read", D: "DB write", E: "transaction", F: "auth + DB read" };
const order = ["usai", "node", "bun", "deno", "rust", "php"];
const by = (a, b) => order.indexOf(a.server) - order.indexOf(b.server) || a.clients - b.clients;
console.log(`# Benchmark suite — ${dir.split("/").pop()}\n`);
console.log("Performance scoreboard (each server does the same validated work; see BENCHMARKS.md).\n");
for (const [cls, title] of Object.entries(classes)) {
  const rows = cells.filter((c) => c.class === cls).sort(by);
  if (!rows.length) continue;
  console.log(`## ${cls} — ${title}\n`);
  console.log("| server | c | req/s | p50 ms | p95 ms | p99 ms | CPU ms/req | RSS MiB | 4xx | 5xx | errors |");
  console.log("|---|---|---|---|---|---|---|---|---|---|---|");
  for (const r of rows) console.log(`| ${r.server} | ${r.clients} | ${r.rps} | ${r.p50} | ${r.p95} | ${r.p99} | ${r.cpuMsPerRequest ?? "—"} | ${r.rssMiB} | ${r.s4xx} | ${r.s5xx} | ${r.errors} |`);
  console.log();
  const inv = rows.find((r) => r.server === "usai" && r.invoice);
  if (inv) {
    const i = inv.invoice;
    const g = (k) => i[k] ?? 0;
    const host = ["route", "decode", "validate", "admit", "encode"].reduce((s, k) => s + g(`http.${k}`), 0);
    const guest = Object.entries(i).filter(([k]) => k.startsWith("guest.")).map(([k, v]) => `${k.slice(6)} ${v.toFixed(3)}`).join(", ");
    console.log(`Usai invoice at c=1 (ms): host HTTP ${host.toFixed(3)} · execute ${g("http.execute").toFixed(3)} = create ${g("runtime.create").toFixed(3)} + run ${g("driver.run").toFixed(3)} + release ${(g("http.execute") - g("runtime.create") - g("driver.run") - g("driver.retire")).toFixed(3)} · engine: entry ${(g("engine.invoke.entry") + g("engine.invoke.eval")).toFixed(3)}, invoke.jobs ${g("engine.invoke.jobs").toFixed(3)}, state ${(g("engine.state") + g("engine.outcome.eval") + g("engine.pending.eval")).toFixed(3)} · guest: ${guest}\n`);
  }
}
