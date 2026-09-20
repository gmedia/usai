import assert from "node:assert/strict";
import { test } from "node:test";
import { parseCookies, serializeCookie, signCookieValue, verifyCookieValue } from "./cookies.ts";

test("parseCookies: first occurrence wins, quotes and encoding handled, junk skipped", () => {
  assert.deepEqual(parseCookies('a=1; b="two"; a=3; c=%20x; junk; d='), {
    a: "1",
    b: "two",
    c: " x",
    d: "",
  });
  assert.deepEqual(parseCookies(undefined), {});
});

test("serializeCookie: safe defaults, round-trips through parse, refuses bad shapes", () => {
  const set = serializeCookie("sid", "a b;c", { maxAge: 60 });
  assert.equal(set, "sid=a%20b%3Bc; Max-Age=60; Path=/; Secure; HttpOnly; SameSite=Lax");
  assert.equal(parseCookies(set.split(";")[0]).sid, "a b;c");
  assert.equal(
    serializeCookie("t", "x", { httpOnly: false, secure: false, sameSite: "Strict", path: "/app" }),
    "t=x; Path=/app; SameSite=Strict",
  );
  assert.equal(
    serializeCookie("sid", "", { maxAge: 0 }),
    "sid=; Max-Age=0; Path=/; Secure; HttpOnly; SameSite=Lax",
  );
  assert.throws(() => serializeCookie("bad name", "x"), TypeError);
  assert.throws(() => serializeCookie("s", "x", { sameSite: "None", secure: false }), TypeError);
  assert.throws(() => serializeCookie("s", "x", { maxAge: -1 }), TypeError);
});

test("sign/verify: tamper-proof, rotation-friendly, constant shape", async () => {
  const signed = await signCookieValue("user-42", "secret-a");
  assert.match(signed, /^user-42\.[A-Za-z0-9_-]{43}$/);
  assert.equal(await verifyCookieValue(signed, "secret-a"), "user-42");
  assert.equal(await verifyCookieValue(signed, "secret-b"), null);
  assert.equal(
    await verifyCookieValue(signed, "secret-b", "secret-a"),
    "user-42",
    "rotation: any listed secret",
  );
  assert.equal(
    await verifyCookieValue(`user-43.${signed.split(".")[1]}`, "secret-a"),
    null,
    "tampered value",
  );
  assert.equal(await verifyCookieValue("no-dot", "secret-a"), null);
  assert.equal(await verifyCookieValue(undefined, "secret-a"), null);
  await assert.rejects(signCookieValue("a.b", "s"), TypeError);
});
