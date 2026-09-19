#!/usr/bin/env node
// Every comparator must do the same work before it is measured: the same
// status and the same JSON (parsed, key order ignored) for the 200/201, 400,
// 401, 404 and 409 paths of every class. A server that deviates is not
// measured. Usage: conformance.mjs <base> [--strict-messages]
const base = process.argv[2];
if (!base) { console.error("usage: conformance.mjs <base>"); process.exit(2); }
const ref = "6f1a2b3c-4d5e-4f60-8a71-92b3c4d5e6f7";
const quote = { customer: "Ayu", email: "ayu@example.com", currency: "IDR", country: "ID", requestedDate: "2026-10-01", reference: ref, tags: ["a"], items: [{ sku: "A1", quantity: 2, unitCents: 1500 }] };
const suffix = Date.now().toString(36);
let failures = 0;
const check = async (name, method, path, init, expectStatus, expectBody) => {
  const res = await fetch(base + path, { method, ...init });
  const text = await res.text();
  let body = null;
  try { body = JSON.parse(text); } catch { /* not JSON */ }
  const problems = [];
  if (res.status !== expectStatus) problems.push(`status ${res.status} ≠ ${expectStatus}`);
  if (expectBody !== undefined) {
    const got = typeof expectBody === "function" ? expectBody(body) : sameShape(expectBody, body);
    if (got !== true) problems.push(typeof got === "string" ? got : `body ${text.slice(0, 200)}`);
  }
  if (problems.length) { failures++; console.log(`✗ ${name}: ${problems.join("; ")}`); } else console.log(`✓ ${name}`);
};
// Equal values, keys in any order; `error.message` is compared by presence only.
function sameShape(expected, actual) {
  if (expected === null || typeof expected !== "object") return Object.is(expected, actual) || `expected ${JSON.stringify(expected)}, got ${JSON.stringify(actual)}`;
  if (Array.isArray(expected)) { if (!Array.isArray(actual) || actual.length !== expected.length) return `array ${JSON.stringify(actual)}`; for (let i = 0; i < expected.length; i++) { const r = sameShape(expected[i], actual[i]); if (r !== true) return r; } return true; }
  if (actual === null || typeof actual !== "object") return `expected object, got ${JSON.stringify(actual)}`;
  for (const k of Object.keys(expected)) {
    if (k === "message" && typeof actual[k] === "string") continue;
    const r = sameShape(expected[k], actual[k]); if (r !== true) return `${k}: ${r}`;
  }
  return true;
}
const json = (v) => ({ headers: { "content-type": "application/json" }, body: JSON.stringify(v) });
const error = (code) => ({ error: { code, message: "" } });

await check("A hello 200", "GET", "/hello/ayu", {}, 200, { hello: "ayu" });
await check("A hello 400 (too long)", "GET", "/hello/" + "x".repeat(41), {}, 400, (b) => b?.error?.code === "validation_failed" || `code ${b?.error?.code}`);
await check("B quote 201", "POST", "/orders/quote", json(quote), 201, { reference: ref, currency: "IDR", subtotalCents: 3000, taxCents: 330, totalCents: 3330, lines: 1, priority: "normal" });
await check("B quote 400 (bad email)", "POST", "/orders/quote", json({ ...quote, email: "nope" }), 400, (b) => b?.error?.code === "validation_failed" || `code ${b?.error?.code}`);
await check("B quote 400 (no items)", "POST", "/orders/quote", json({ ...quote, items: [] }), 400, (b) => b?.error?.code === "validation_failed" || `code ${b?.error?.code}`);
await check("C user 200", "GET", "/users/42", {}, 200, { id: 42, name: "user-42", email: "user42@example.test" });
await check("C user 404", "GET", "/users/2000000000", {}, 404, error("not_found"));
await check("C user 400", "GET", "/users/abc", {}, 400, (b) => b?.error?.code === "validation_failed" || `code ${b?.error?.code}`);
await check("D create 201", "POST", "/users", json({ name: "Conf", email: `conf-${suffix}@example.test` }), 201, (b) => (typeof b?.id === "number" && b.name === "Conf") || `body ${JSON.stringify(b)}`);
await check("D create 409", "POST", "/users", json({ name: "Conf", email: `conf-${suffix}@example.test` }), 409, error("conflict"));
await check("D create 400", "POST", "/users", json({ name: "", email: "x" }), 400, (b) => b?.error?.code === "validation_failed" || `code ${b?.error?.code}`);
const orderId = 90_000 + (Date.now() % 9_000);
await check("E pay 200", "POST", `/orders/${orderId}/pay`, {}, 200, (b) => (b?.orderId === orderId && b.paid === true && typeof b.paymentId === "number" && typeof b.amountCents === "number") || `body ${JSON.stringify(b)}`);
await check("E pay 409 (already paid)", "POST", `/orders/${orderId}/pay`, {}, 409, error("conflict"));
await check("E pay 404", "POST", "/orders/2000000000/pay", {}, 404, error("not_found"));
await check("F me 200", "GET", "/me", { headers: { "x-api-key": "key-5" } }, 200, { id: 5, name: "user-5", email: "user5@example.test" });
await check("F me 401 (missing)", "GET", "/me", {}, 401, error("unauthorized"));
await check("F me 401 (unknown)", "GET", "/me", { headers: { "x-api-key": "nope" } }, 401, error("unauthorized"));
await check("route 404", "GET", "/nothing/here", {}, 404, error("route_not_found"));
await check("probe counter", "GET", "/counter", {}, 200, (b) => typeof b?.count === "number" || `body ${JSON.stringify(b)}`);
await check("probe slow", "GET", "/slow?ms=10", {}, 200, { slept: 10 });
console.log(failures ? `${failures} deviation(s): not measured` : "conformant");
process.exit(failures ? 1 : 0);
