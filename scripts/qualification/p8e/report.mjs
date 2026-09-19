#!/usr/bin/env node
// Renders a P8E run directory (fleet.sh) as markdown: per cell, the phases
// with the fleet's memory (RSS and PSS summed over its processes), CPU,
// wakeups and fds, plus the load results and the verdict inputs.
//
//   report.mjs <run_dir>
import { existsSync, readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";

const dir = process.argv[2];
if (!dir) {
  console.error("report.mjs <run_dir>");
  process.exit(2);
}
const jsonl = (p) =>
  existsSync(p)
    ? readFileSync(p, "utf8")
        .split("\n")
        .filter(Boolean)
        .map((l) => JSON.parse(l))
    : [];
const json = (p) => (existsSync(p) ? JSON.parse(readFileSync(p, "utf8")) : null);
const mib = (kib) => (kib / 1024).toFixed(1);
const mean = (xs) => (xs.length ? xs.reduce((a, b) => a + b, 0) / xs.length : 0);
const max = (xs) => (xs.length ? Math.max(...xs) : 0);

// Aggregates the samples that fall inside [from, to].
function window(samples, from, to) {
  const inside = samples.filter((s) => s.t >= from && s.t <= to);
  if (inside.length < 2) return null;
  const sumOver = (key) => inside.map((s) => s.procs.reduce((a, p) => a + p[key], 0));
  const first = inside[0];
  const last = inside[inside.length - 1];
  const seconds = last.t - first.t;
  const ticks =
    last.procs.reduce((a, p) => a + p.cpuTicks, 0) -
    first.procs.reduce((a, p) => a + p.cpuTicks, 0);
  const cpuPct = seconds > 0 ? (ticks / first.hz / seconds) * 100 : 0;
  const ctxtPerS = seconds > 0 ? (last.ctxt - first.ctxt) / seconds : 0;
  const minflt =
    last.procs.reduce((a, p) => a + p.minflt, 0) - first.procs.reduce((a, p) => a + p.minflt, 0);
  return {
    n: inside.length,
    procs: first.procs.length,
    rssMeanKib: mean(sumOver("rssKib")),
    rssMaxKib: max(sumOver("rssKib")),
    pssMeanKib: mean(sumOver("pssKib")),
    pssMaxKib: max(sumOver("pssKib")),
    fds: mean(sumOver("fds")),
    threads: mean(sumOver("threads")),
    cpuPct,
    ctxtPerS,
    minfltPerS: seconds > 0 ? minflt / seconds : 0,
    memAvailMinMib: Math.min(...inside.map((s) => s.memAvailableKib)) / 1024,
  };
}

function loadSummary(p) {
  const o = json(p);
  if (!o) return "";
  if (o.summary && o.metrics) {
    // oha
    const codes = o.statusCodeDistribution || {};
    const bad = Object.entries(codes)
      .filter(([k]) => Number(k) >= 400)
      .reduce((a, [, v]) => a + v, 0);
    return `${o.summary.requestsPerSec.toFixed(0)} req/s, p50 ${(o.metrics.latency_ms?.p50 ?? o.latencyPercentiles?.p50 * 1000).toFixed(2)} ms, p99 ${(o.metrics.latency_ms?.p99 ?? o.latencyPercentiles?.p99 * 1000).toFixed(2)} ms, ≥400: ${bad}`;
  }
  if (o.rps !== undefined)
    return `${o.rps} req/s, p50 ${o.p50} ms, p99 ${o.p99} ms, 4xx ${o.s4xx} 5xx ${o.s5xx} err ${o.errors}`;
  return "";
}

const cells = readdirSync(dir).filter((d) => d.startsWith("floor-") || d.startsWith("density-"));
console.log(`# P8E run ${dir}\n`);
for (const cell of cells.sort()) {
  const d = join(dir, cell);
  const samples = jsonl(join(d, "samples.jsonl"));
  const phases = jsonl(join(d, "phases.jsonl"));
  const result = json(join(d, "result.json")) || {};
  console.log(`## ${cell}\n`);
  console.log(
    `result: ${JSON.stringify(result)}${existsSync(join(d, "timing.txt")) ? ` · ${readFileSync(join(d, "timing.txt"), "utf8").trim()}` : ""}${existsSync(join(d, "cold.json")) ? ` · ${JSON.stringify(json(join(d, "cold.json")))}` : ""}\n`,
  );
  if (!samples.length) {
    console.log("(no samples)\n");
    continue;
  }
  console.log(
    "| phase | s | procs | RSS Σ mean / max MiB | PSS Σ mean / max MiB | CPU % | ctxt/s (host) | minflt/s | fds | threads | MemAvailable min MiB |",
  );
  console.log("|---|---|---|---|---|---|---|---|---|---|---|");
  const rows = phases.length
    ? phases
    : [{ phase: "all", args: "", from: samples[0].t, to: samples[samples.length - 1].t }];
  for (const ph of rows) {
    const w = window(samples, ph.from, ph.to);
    if (!w) continue;
    console.log(
      `| ${ph.phase} ${ph.args ?? ""} | ${(ph.to - ph.from).toFixed(0)} | ${w.procs} | ${mib(w.rssMeanKib)} / ${mib(w.rssMaxKib)} | ${mib(w.pssMeanKib)} / ${mib(w.pssMaxKib)} | ${w.cpuPct.toFixed(2)} | ${w.ctxtPerS.toFixed(0)} | ${w.minfltPerS.toFixed(0)} | ${w.fds.toFixed(0)} | ${w.threads.toFixed(0)} | ${w.memAvailMinMib.toFixed(0)} |`,
    );
  }
  // Per-process marginal cost in a density cell: the fleet's idle PSS / n.
  const settle = phases.find((p) => p.phase === "settle" || p.phase === "idle");
  if (settle && result.n) {
    const w = window(samples, settle.from, settle.to);
    if (w)
      console.log(
        `\nidle per process: RSS ${mib(w.rssMeanKib / result.n)} MiB, PSS ${mib(w.pssMeanKib / result.n)} MiB, CPU ${(w.cpuPct / result.n).toFixed(3)} %`,
      );
  }
  const loads = readdirSync(d).filter((f) => f.startsWith("load-") || f.startsWith("burst-"));
  for (const f of loads.sort()) {
    const s = loadSummary(join(d, f));
    if (s) console.log(`\n${f}: ${s}`);
  }
  console.log();
}
