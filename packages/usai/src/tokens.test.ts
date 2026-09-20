import assert from "node:assert/strict";
import { test } from "node:test";
import { tokens } from "./tokens.ts";

const secret = "s3cret-for-tests";

test("a signed token verifies, carries its claims, and expires", async () => {
  const now = 1_700_000_000_000;
  const token = await tokens.sign({ sub: "u1", role: "admin" }, secret, { expiresIn: "15m", now });
  assert.match(token, /^[A-Za-z0-9_-]+\.[A-Za-z0-9_-]{43}$/);
  const claims = await tokens.verify<{ sub: string; role: string }>(token, secret, { now });
  assert.deepEqual(claims, { sub: "u1", role: "admin", iat: 1_700_000_000, exp: 1_700_000_900 });
  assert.equal(await tokens.verify(token, secret, { now: now + 15 * 60_000 }), null, "expired");
});

test("verification refuses tampering, other secrets and malformed input", async () => {
  const token = await tokens.sign({ sub: "u1" }, secret, { expiresIn: 60 });
  const [body, sig] = token.split(".") as [string, string];
  const other = await tokens.sign({ sub: "u2" }, secret, { expiresIn: 60 });
  const [otherBody] = other.split(".") as [string];
  assert.equal(await tokens.verify(`${otherBody}.${sig}`, secret), null, "swapped payload");
  assert.equal(
    await tokens.verify(`${body}.${sig.slice(0, -1)}A`, secret),
    null,
    "altered signature",
  );
  assert.equal(await tokens.verify(token, "another-secret"), null);
  assert.equal(
    await tokens.verify(`${token}\n`, secret),
    null,
    "trailing newline is not the token",
  );
  for (const bad of ["", "abc", "a.b.c", ".sig", "body.", undefined, null]) {
    assert.equal(await tokens.verify(bad, secret), null, JSON.stringify(bad));
  }
  await assert.rejects(tokens.verify(token, ""), /secret is required/);
});

test("rotation: several secrets verify, the newest signs", async () => {
  const old = await tokens.sign({ sub: "u1" }, "old", { expiresIn: "1h" });
  assert.ok(await tokens.verify(old, ["new", "old"]));
  assert.equal(await tokens.verify(old, ["new"]), null);
});

test("sign refuses a missing secret, a non-positive lifetime and reserved claims", async () => {
  await assert.rejects(tokens.sign({ a: 1 }, "", { expiresIn: "1m" }), /secret is required/);
  await assert.rejects(tokens.sign({ a: 1 }, secret, { expiresIn: 0 }), /positive/);
  await assert.rejects(tokens.sign({ exp: 1 }, secret, { expiresIn: "1m" }), /iat and exp/);
});
