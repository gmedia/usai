import { test } from "node:test";
import assert from "node:assert/strict";
import { execFile, execFileSync, spawnSync } from "node:child_process";
import { promisify } from "node:util";
import { createHash } from "node:crypto";
import { chmodSync, existsSync, mkdirSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
// The fake release server lives in this process, so the wrapper must run
// asynchronously or the two deadlock.
const run = async (env: NodeJS.ProcessEnv) => {
  try {
    const r = await promisify(execFile)(process.execPath, [wrapper, "--version"], { env, encoding: "utf8" });
    return { status: 0, ...r };
  } catch (error) {
    const e = error as { code?: number; stdout: string; stderr: string };
    return { status: e.code ?? 1, stdout: e.stdout, stderr: e.stderr };
  }
};
const wrapper = resolve(here, "..", "bin", "usai.mjs");
const { version } = JSON.parse(readFileSync(resolve(here, "..", "package.json"), "utf8")) as { version: string };
const { target, cacheDir, releaseUrl } = await import("../bin/usai.mjs");

test("release targets: the three published triples, nothing else", () => {
  assert.equal(target("linux", "x64"), "x86_64-unknown-linux-gnu");
  assert.equal(target("linux", "arm64"), "aarch64-unknown-linux-gnu");
  assert.equal(target("darwin", "arm64"), "aarch64-apple-darwin");
  assert.equal(target("win32", "x64"), null);
  assert.equal(target("darwin", "x64"), null);
});

test("cache dir and release URL follow the environment", () => {
  assert.equal(cacheDir({ USAI_CACHE_DIR: "/x/y" }), "/x/y");
  assert.equal(cacheDir({ XDG_CACHE_HOME: "/xdg" }), "/xdg/usai");
  assert.match(releaseUrl("a.tar.gz", {}), new RegExp(`^https://github.com/gmedia/usai/releases/download/v${version.replaceAll(".", "\\.")}/a\\.tar\\.gz$`));
  assert.equal(releaseUrl("a.tar.gz", { USAI_RELEASE_BASE: "http://mirror/" }), "http://mirror/a.tar.gz");
});

test("USAI_BIN is used as is; a same-version usai on PATH wins over a fetch", () => {
  const dir = mkdtempSync(join(tmpdir(), "usai-bin-"));
  const fake = join(dir, "usai");
  writeFileSync(fake, `#!/bin/sh\nif [ "$1" = "--version" ]; then echo "usai ${version}"; else echo "args: $@"; fi\n`);
  chmodSync(fake, 0o755);
  const explicit = execFileSync(process.execPath, [wrapper, "dev", "--port", "1"], { env: { ...process.env, USAI_BIN: fake }, encoding: "utf8" });
  assert.match(explicit, /args: dev --port 1/);
  const onPath = execFileSync(process.execPath, [wrapper, "--version"], {
    env: { ...process.env, USAI_BIN: "", PATH: `${dir}:${process.env["PATH"]}`, USAI_CACHE_DIR: join(dir, "never") },
    encoding: "utf8",
  });
  assert.match(onPath, new RegExp(`usai ${version.replaceAll(".", "\\.")}`));
  assert.ok(!existsSync(join(dir, "never")), "nothing fetched when PATH has the right version");
});

test("first use fetches the release tarball, verifies its SHA-256, caches the binary; a bad sum is refused", async () => {
  const triple = target(process.platform, process.arch);
  if (!triple || spawnSync("tar", ["--version"]).status !== 0) return;
  const dir = mkdtempSync(join(tmpdir(), "usai-fetch-"));
  const name = `usai-v${version}-${triple}`;
  mkdirSync(join(dir, name));
  writeFileSync(join(dir, name, "usai"), `#!/bin/sh\necho "usai ${version} (fetched)"\n`);
  execFileSync("tar", ["-C", dir, "-czf", join(dir, `${name}.tar.gz`), name]);
  const tarball = readFileSync(join(dir, `${name}.tar.gz`));
  const sum = createHash("sha256").update(tarball).digest("hex");
  let corrupt = false;
  const server = createServer((req, res) => {
    if (req.url?.endsWith(".sha256")) res.end(`${corrupt ? "0".repeat(64) : sum}  ${name}.tar.gz\n`);
    else if (req.url?.endsWith(".tar.gz")) res.end(tarball);
    else res.writeHead(404).end();
  });
  await new Promise<void>((ok) => server.listen(0, "127.0.0.1", ok));
  const port = (server.address() as { port: number }).port;
  const env = { ...process.env, USAI_BIN: "", PATH: "/usr/bin:/bin", USAI_CACHE_DIR: join(dir, "cache"), USAI_RELEASE_BASE: `http://127.0.0.1:${port}` };
  try {
    corrupt = true;
    const refused = await run(env);
    assert.notEqual(refused.status, 0);
    assert.match(refused.stderr, /SHA-256 mismatch/);
    assert.ok(!existsSync(join(dir, "cache", version, "usai")));
    corrupt = false;
    const first = await run(env);
    assert.equal(first.status, 0, first.stderr);
    assert.match(first.stderr, /fetching/);
    assert.match(first.stdout, /\(fetched\)/);
    const second = await run(env);
    assert.equal(second.stderr, "", "cached: no fetch line");
    assert.match(second.stdout, /\(fetched\)/);
  } finally {
    server.closeAllConnections();
    server.close();
  }
});

test("under `pnpm usai` the wrapper is itself on PATH and must not probe itself", () => {
  // node_modules/.bin/usai → the wrapper, exactly what pnpm puts first on PATH.
  const dir = mkdtempSync(join(tmpdir(), "usai-self-"));
  const bin = join(dir, "node_modules", ".bin");
  mkdirSync(bin, { recursive: true });
  writeFileSync(join(bin, "usai"), `#!/bin/sh\nexec "${process.execPath}" "${wrapper}" "$@"\n`);
  chmodSync(join(bin, "usai"), 0o755);
  const r = spawnSync(join(bin, "usai"), ["--version"], {
    env: { ...process.env, USAI_BIN: "", PATH: `${bin}:/usr/bin:/bin`, USAI_CACHE_DIR: join(dir, "cache"), USAI_RELEASE_BASE: "http://127.0.0.1:9" },
    encoding: "utf8",
    timeout: 20_000,
  });
  assert.notEqual(r.signal, "SIGTERM", "the wrapper hung (recursion)");
  assert.notEqual(r.status, 0);
  assert.match(r.stderr, /fetching|ECONNREFUSED|fetch failed/);
});
