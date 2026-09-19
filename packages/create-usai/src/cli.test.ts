import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, existsSync, symlinkSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { scaffold } from "./cli.ts";

test("scaffolds the hello template with the project name substituted", () => {
  const dir = join(mkdtempSync(join(tmpdir(), "create-usai-")), "My App");
  const target = scaffold({ target: dir, usaiVersion: "0.0.1" });
  assert.ok(existsSync(join(target, "src/app.ts")));
  assert.ok(existsSync(join(target, "usai.config.ts")));
  const pkg = JSON.parse(readFileSync(join(target, "package.json"), "utf8"));
  assert.equal(pkg.name, "my-app");
  assert.equal(pkg.dependencies["@sakaladev/usai"], "^0.0.1");
  assert.match(readFileSync(join(target, "src/app.ts"), "utf8"), /name: "my-app"/);
  assert.ok(existsSync(join(target, ".gitignore")) && !existsSync(join(target, "_gitignore")));
  assert.ok(existsSync(join(target, ".env.example")));
  assert.ok(existsSync(join(target, "pnpm-workspace.yaml")));
  assert.ok(
    existsSync(join(target, ".dockerignore")) &&
      existsSync(join(target, "compose.yaml")) &&
      existsSync(join(target, "Dockerfile")),
  );
  const compose = readFileSync(join(target, "compose.yaml"), "utf8");
  assert.match(compose, /sakaladev\/usai:0\.0\.1-dev/);
  assert.match(
    compose,
    new RegExp(`user: "${process.getuid?.() ?? 1000}:${process.getgid?.() ?? 1000}"`),
  );
  assert.match(
    readFileSync(join(target, "Dockerfile"), "utf8"),
    /FROM sakaladev\/usai:0\.0\.1\s*$/m,
  );
  assert.throws(() => scaffold({ target: dir }), /not empty/);
});

test("runs as an executable through a symlink, the way package managers link bins", () => {
  const dist = resolve(import.meta.dirname, "../dist/cli.js");
  if (!existsSync(dist)) return; // built by `pnpm run build`; CI builds before testing
  const work = mkdtempSync(join(tmpdir(), "create-usai-bin-"));
  const link = join(work, "create-usai");
  symlinkSync(dist, link);
  const out = execFileSync(process.execPath, [link, "my-app"], { cwd: work, encoding: "utf8" });
  assert.match(out, /created/);
  assert.ok(existsSync(join(work, "my-app/src/app.ts")));
  // The scaffold depends on the SDK's version, not the scaffolder's own.
  const pkg = JSON.parse(readFileSync(join(work, "my-app/package.json"), "utf8"));
  const own = JSON.parse(readFileSync(resolve(import.meta.dirname, "../package.json"), "utf8"));
  assert.equal(pkg.dependencies["@sakaladev/usai"], `^${own.sdkVersion}`);
});
