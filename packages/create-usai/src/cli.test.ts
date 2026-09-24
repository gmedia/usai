import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, existsSync, symlinkSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { scaffold } from "./cli.ts";

test("scaffolds the hello template with the project name substituted", () => {
  const dir = join(mkdtempSync(join(tmpdir(), "create-usai-")), "My App");
  const { target, version } = scaffold({ target: dir, usaiVersion: "0.0.1" });
  // What it pinned comes back, so the CLI can print it — a returning user
  // whose `pnpm dlx` cache handed them an older scaffolder sees the wrong
  // version on screen instead of finding it weeks later.
  assert.equal(version, "0.0.1");
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

test("the scaffolder prints the SDK version it pinned", () => {
  // Measured on 2026-09-24 with `@sakaladev/usai@0.0.10` published:
  // `pnpm dlx @sakaladev/create-usai` scaffolded `^0.0.8` and
  // `pnpm dlx @sakaladev/create-usai@latest` scaffolded `^0.0.4` on a
  // machine that had run it before, while `npx -y` on the same machine
  // scaffolded `^0.0.10`. pnpm's dlx cache is keyed by the spec, so even
  // `@latest` hits it. A scaffolder cannot fix that; being silent about
  // which SDK it wrote is what turns it into a bug nobody notices.
  const dir = join(mkdtempSync(join(tmpdir(), "create-usai-print-")), "app");
  const out = execFileSync(
    process.execPath,
    [resolve(import.meta.dirname, "../dist/cli.js"), dir],
    { encoding: "utf8" },
  );
  assert.match(out, /@sakaladev\/usai \^\d+\.\d+\.\d+/, out);
  assert.match(out, /create-usai \d+\.\d+\.\d+/, out);
});

test("runs as an executable through a symlink, the way package managers link bins", () => {
  // `pretest` builds it, so this no longer skips. It used to say "CI builds
  // before testing", which `make test` does not — so the one test that
  // covers how a package manager actually invokes this binary had been
  // silently passing by not running.
  const dist = resolve(import.meta.dirname, "../dist/cli.js");
  assert.ok(existsSync(dist), "pretest should have built dist/cli.js");
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
