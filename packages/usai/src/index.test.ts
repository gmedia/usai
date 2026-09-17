import { test } from "node:test";
import assert from "node:assert/strict";
import { MANIFEST_VERSION } from "./index.ts";

test("manifest version matches the runtime", () => {
  assert.equal(MANIFEST_VERSION, 1);
});
