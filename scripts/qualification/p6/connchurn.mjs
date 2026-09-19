#!/usr/bin/env node
// Connection-bound worlds against the deployment: server-sent events
// (`GET /invoices/live`) and WebSockets (`GET /invoices/socket`). Three
// phases, one JSON summary on stdout at the end; never stops on errors.
//
//   connchurn.mjs <base> <token> cycles <n> <concurrency>
//       n × (SSE: connect, two events, abort; WS: connect, hello, ask, answer, close 1000)
//   connchurn.mjs <base> <token> hold <connections> <seconds>
//       half SSE, half WS, kept open and reading; a probe connection every
//       second; every close the server initiates is recorded with its code,
//       reason and the second it happened — what a revision drain, a
//       restart or a proxy restart looks like from the client's chair
//   connchurn.mjs <base> <token> slow <seconds>
//       one SSE connection whose body is never read (TCP backpressure), then aborted
//   connchurn.mjs <base> <token> idle <seconds>
//       one WS that sends nothing; reports the close the server sends
const [base, token, phase, a1, a2] = process.argv.slice(2);
const headers = { authorization: `Bearer ${token}` };
const ws = (path) => new WebSocket(`${base.replace(/^http/, "ws")}${path}`, { headers });
const pct = (a, p) => { if (!a.length) return null; const s = [...a].sort((x, y) => x - y); return +s[Math.min(s.length - 1, Math.floor(p * s.length))].toFixed(1); };
const started = Date.now();
const sec = () => +((Date.now() - started) / 1000).toFixed(1);

async function sseOnce(events = 2, everyMs = 200) {
  const ctl = new AbortController();
  const t = performance.now();
  const res = await fetch(`${base}/invoices/live?everyMs=${everyMs}`, { headers, signal: ctl.signal });
  if (res.status !== 200) { ctl.abort(); throw new Error(`sse ${res.status}`); }
  const reader = res.body.getReader();
  let text = "", seen = 0;
  while (seen < events) {
    const { value, done } = await reader.read();
    if (done) throw new Error("sse ended early");
    text += new TextDecoder().decode(value);
    seen = text.split("\n\n").length - 1;
  }
  ctl.abort();
  return performance.now() - t;
}
function wsOnce() {
  return new Promise((ok, bad) => {
    const t = performance.now();
    const s = ws("/invoices/socket");
    const got = [];
    const timer = setTimeout(() => { s.close(); bad(new Error(`ws timeout after ${got.length} messages`)); }, 10_000);
    s.addEventListener("open", () => s.send(JSON.stringify({ type: "counts" })));
    s.addEventListener("message", (e) => { got.push(JSON.parse(e.data).type); if (got.length === 2) { clearTimeout(timer); s.close(1000, "done"); ok(performance.now() - t); } });
    s.addEventListener("error", () => { clearTimeout(timer); bad(new Error("ws error")); });
    s.addEventListener("close", (e) => { if (got.length < 2) { clearTimeout(timer); bad(new Error(`ws closed ${e.code} ${e.reason}`)); } });
  });
}

if (phase === "cycles") {
  const n = Number(a1 ?? 500), conc = Number(a2 ?? 16);
  const out = { sse: { ok: 0, errors: {}, ms: [] }, ws: { ok: 0, errors: {}, ms: [] } };
  let next = 0;
  const worker = async () => {
    while (next < n) {
      next++;
      try { out.sse.ms.push(await sseOnce()); out.sse.ok++; } catch (e) { out.sse.errors[e.message] = (out.sse.errors[e.message] ?? 0) + 1; }
      try { out.ws.ms.push(await wsOnce()); out.ws.ok++; } catch (e) { out.ws.errors[e.message] = (out.ws.errors[e.message] ?? 0) + 1; }
    }
  };
  await Promise.all(Array.from({ length: conc }, worker));
  console.log(JSON.stringify({ phase, cycles: n, concurrency: conc, seconds: sec(), sse: { ok: out.sse.ok, errors: out.sse.errors, p50: pct(out.sse.ms, 0.5), p99: pct(out.sse.ms, 0.99) }, ws: { ok: out.ws.ok, errors: out.ws.errors, p50: pct(out.ws.ms, 0.5), p99: pct(out.ws.ms, 0.99) } }));
} else if (phase === "hold") {
  const count = Number(a1 ?? 100), seconds = Number(a2 ?? 60);
  const closes = [], probes = [];
  let events = 0, messages = 0, opened = 0, openErrors = 0;
  const holders = [];
  for (let i = 0; i < count / 2; i++) {
    holders.push((async () => {
      const ctl = new AbortController();
      setTimeout(() => ctl.abort(), seconds * 1000);
      try {
        const res = await fetch(`${base}/invoices/live?everyMs=1000`, { headers, signal: ctl.signal });
        if (res.status !== 200) { openErrors++; return; }
        opened++;
        const reader = res.body.getReader();
        for (;;) { const { value, done } = await reader.read(); if (done) { closes.push({ kind: "sse", at: sec(), how: "ended" }); return; } events += new TextDecoder().decode(value).split("\n\n").length - 1; }
      } catch (e) { if (e.name !== "AbortError") closes.push({ kind: "sse", at: sec(), how: e.message }); }
    })());
    holders.push(new Promise((done) => {
      const s = ws("/invoices/socket");
      const ping = setInterval(() => { if (s.readyState === 1) s.send(JSON.stringify({ type: "counts" })); }, 1000);
      const end = setTimeout(() => { clearInterval(ping); if (s.readyState <= 1) s.close(1000, "hold over"); }, seconds * 1000);
      s.addEventListener("open", () => opened++);
      s.addEventListener("message", () => messages++);
      s.addEventListener("error", () => { openErrors++; });
      s.addEventListener("close", (e) => { clearInterval(ping); clearTimeout(end); if (e.reason !== "hold over") closes.push({ kind: "ws", at: sec(), code: e.code, reason: e.reason }); done(); });
    }));
  }
  const probe = setInterval(async () => {
    const t = sec();
    try { await sseOnce(1, 200); await wsOnce(); probes.push({ at: t, ok: true }); } catch (e) { probes.push({ at: t, ok: false, error: e.message }); }
  }, 1000);
  await Promise.all(holders);
  clearInterval(probe);
  const byReason = {};
  for (const c of closes) { const k = `${c.kind}:${c.code ?? c.how ?? ""}:${c.reason ?? ""}`; byReason[k] = (byReason[k] ?? 0) + 1; }
  const firstClose = closes.length ? Math.min(...closes.map((c) => c.at)) : null, lastClose = closes.length ? Math.max(...closes.map((c) => c.at)) : null;
  const failedProbes = probes.filter((p) => !p.ok);
  console.log(JSON.stringify({ phase, connections: count, seconds, opened, openErrors, events, messages, serverCloses: closes.length, byReason, firstCloseAt: firstClose, lastCloseAt: lastClose, probes: probes.length, failedProbes: failedProbes.length, failedProbeSeconds: failedProbes.map((p) => p.at), probeErrors: [...new Set(failedProbes.map((p) => p.error))] }));
  process.exit(0);
} else if (phase === "slow") {
  const seconds = Number(a1 ?? 30);
  const ctl = new AbortController();
  const res = await fetch(`${base}/invoices/live?everyMs=200`, { headers, signal: ctl.signal });
  // Never read the body: the server's sends back up in the socket.
  await new Promise((r) => setTimeout(r, seconds * 1000));
  ctl.abort();
  console.log(JSON.stringify({ phase, status: res.status, seconds, unread: true }));
} else if (phase === "idle") {
  const seconds = Number(a1 ?? 30);
  await new Promise((done) => {
    const s = ws("/invoices/socket");
    const t = Date.now();
    const timer = setTimeout(() => { console.log(JSON.stringify({ phase, closed: false, waited: seconds })); s.close(); done(); }, seconds * 1000);
    s.addEventListener("close", (e) => { clearTimeout(timer); console.log(JSON.stringify({ phase, closed: true, code: e.code, reason: e.reason, afterSeconds: +((Date.now() - t) / 1000).toFixed(1) })); done(); });
  });
} else {
  console.error("phase: cycles | hold | slow | idle");
  process.exit(2);
}
