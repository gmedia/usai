#!/usr/bin/env node
// `usai` from npm: runs the Usai runtime binary at exactly this package's
// version, fetching it from the GitHub release on first use.
//
//   pnpm usai dev        (or `pnpm dev` through the scaffold's scripts)
//
// Resolution order, so the native binary stays first-class:
//   1. $USAI_BINARY                        — an explicit binary, used as is
//   2. `usai` on PATH at the same version  — already installed, nothing fetched
//   3. ~/.cache/usai/<version>/usai        — fetched once, SHA-256 verified
//      ($USAI_CACHE_DIR / $XDG_CACHE_HOME respected; $USAI_RELEASE_BASE for a mirror)
// The release contract keeps runtime and SDK versions equal (one tag publishes
// both), so the version in package.json is the one artifact built by this
// SDK is guaranteed to load.
import { spawn, spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { chmodSync, existsSync, mkdirSync, mkdtempSync, readFileSync, realpathSync, renameSync, rmSync } from "node:fs";
import { homedir, tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const { version } = JSON.parse(readFileSync(join(here, "..", "package.json"), "utf8"));
const REPO = "gmedia/usai";

/** Release target triple for this platform, or null when there is no binary. */
export function target(platform = process.platform, arch = process.arch) {
  if (platform === "linux" && arch === "x64") return "x86_64-unknown-linux-gnu";
  if (platform === "linux" && arch === "arm64") return "aarch64-unknown-linux-gnu";
  if (platform === "darwin" && arch === "arm64") return "aarch64-apple-darwin";
  return null;
}

export function cacheDir(env = process.env) {
  if (env.USAI_CACHE_DIR) return resolve(env.USAI_CACHE_DIR);
  const base = env.XDG_CACHE_HOME ? resolve(env.XDG_CACHE_HOME) : join(homedir(), ".cache");
  return join(base, "usai");
}

/** Where release assets are fetched from; USAI_RELEASE_BASE points a mirror. */
export function releaseUrl(name, env = process.env) {
  const base = env.USAI_RELEASE_BASE ?? `https://github.com/${REPO}/releases/download/v${version}`;
  return `${base.replace(/\/$/, "")}/${name}`;
}

/** `usai` on PATH when it reports exactly this version. */
function onPath() {
  const r = spawnSync("usai", ["--version"], { encoding: "utf8" });
  if (r.status !== 0) return null;
  return r.stdout.trim() === `usai ${version}` ? "usai" : null;
}

async function fetchBytes(url) {
  const res = await fetch(url, { redirect: "follow" });
  if (!res.ok) throw new Error(`${url}: HTTP ${res.status}`);
  return Buffer.from(await res.arrayBuffer());
}

async function download(triple) {
  const dir = join(cacheDir(), version);
  const bin = join(dir, "usai");
  if (existsSync(bin)) return bin;
  const name = `usai-v${version}-${triple}`;
  process.stderr.write(`usai: fetching ${name} (once per version) …\n`);
  const [tarball, sums] = await Promise.all([fetchBytes(releaseUrl(`${name}.tar.gz`)), fetchBytes(releaseUrl(`${name}.tar.gz.sha256`))]);
  const expected = sums.toString("utf8").trim().split(/\s+/)[0];
  const actual = createHash("sha256").update(tarball).digest("hex");
  if (expected !== actual) throw new Error(`${name}.tar.gz: SHA-256 mismatch (expected ${expected}, got ${actual})`);
  mkdirSync(dir, { recursive: true });
  const work = mkdtempSync(join(tmpdir(), "usai-"));
  try {
    const tar = spawnSync("tar", ["-xzf", "-", "-C", work], { input: tarball, encoding: "utf8" });
    if (tar.status !== 0) throw new Error(`tar failed: ${tar.error?.message ?? tar.stderr.trim()}`);
    chmodSync(join(work, name, "usai"), 0o755);
    renameSync(join(work, name, "usai"), bin);
  } finally {
    rmSync(work, { recursive: true, force: true });
  }
  return bin;
}

export async function binary() {
  if (process.env.USAI_BINARY) return process.env.USAI_BINARY;
  const found = onPath();
  if (found) return found;
  const triple = target();
  if (!triple) {
    throw new Error(
      `no prebuilt usai binary for ${process.platform}-${process.arch}. ` +
        `Build one with \`cargo build --release -p usai-cli\` and set USAI_BINARY, ` +
        `or use the Docker path (docker compose up). See https://github.com/${REPO}#install.`,
    );
  }
  return download(triple);
}

async function main() {
  const bin = await binary();
  const child = spawn(bin, process.argv.slice(2), { stdio: "inherit" });
  // Ctrl-C reaches the child through the process group already; forwarding
  // it too would be the "second signal" that forces the exit. SIGTERM/SIGHUP
  // sent to this wrapper (docker stop, a supervisor) are forwarded once.
  process.on("SIGINT", () => {});
  for (const signal of ["SIGTERM", "SIGHUP"]) process.on(signal, () => child.kill(signal));
  child.on("exit", (code, signal) => process.exit(code ?? (signal ? 128 + 1 : 1)));
  child.on("error", (error) => {
    process.stderr.write(`usai: ${error.message}\n`);
    process.exit(1);
  });
}

const invoked = process.argv[1] && existsSync(process.argv[1]) ? realpathSync(process.argv[1]) : "";
if (invoked === realpathSync(fileURLToPath(import.meta.url))) {
  main().catch((error) => {
    process.stderr.write(`usai: ${error.message}\n`);
    process.exit(1);
  });
}
