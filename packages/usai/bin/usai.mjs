#!/usr/bin/env node
// `usai` from npm: runs the Usai runtime binary at exactly this package's
// version, fetching it from the GitHub release on first use.
//
//   pnpm usai dev        (or `pnpm dev` through the scaffold's scripts)
//
// Resolution order, so the native binary stays first-class:
//   1. $USAI_BIN                        — an explicit binary, used as is
//   2. `usai` on PATH at the same version  — already installed, nothing fetched
//   3. ~/.cache/usai/<version>/usai        — fetched once, SHA-256 verified
//      ($USAI_CACHE_DIR / $XDG_CACHE_HOME respected; $USAI_RELEASE_BASE for a mirror)
// The release contract keeps runtime and SDK versions equal (one tag publishes
// both), so the version in package.json is the one artifact built by this
// SDK is guaranteed to load.
import { spawn, spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import {
  chmodSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  realpathSync,
  renameSync,
  rmSync,
} from "node:fs";
import { homedir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const { version } = JSON.parse(readFileSync(join(here, "..", "package.json"), "utf8"));
const REPO = "gmedia/usai";
// What the Linux binaries are built for (SUPPORTED.md); used only in the
// diagnostic when the loader refuses one.
const GLIBC_FLOOR = "2.36";

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

/**
 * A native `usai` on PATH that reports exactly this version. Under `pnpm usai`
 * PATH starts with node_modules/.bin, where `usai` is this wrapper: shims that
 * live under node_modules are skipped, and a probe carries USAI_PROBE so a
 * wrapper reached anyway answers nothing instead of probing again.
 */
function onPath(env = process.env) {
  if (env.USAI_PROBE) return null;
  const self = realpathSync(fileURLToPath(import.meta.url));
  for (const dir of (env.PATH ?? "").split(":")) {
    if (!dir) continue;
    const candidate = join(dir, "usai");
    let real;
    try {
      real = realpathSync(candidate);
    } catch {
      continue;
    }
    if (real === self || real.split("/").includes("node_modules")) continue;
    const r = spawnSync(candidate, ["--version"], {
      encoding: "utf8",
      env: { ...env, USAI_PROBE: "1" },
    });
    if (r.status === 0 && r.stdout.trim() === `usai ${version}`) return candidate;
  }
  return null;
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
  const [tarball, sums] = await Promise.all([
    fetchBytes(releaseUrl(`${name}.tar.gz`)),
    fetchBytes(releaseUrl(`${name}.tar.gz.sha256`)),
  ]);
  const expected = sums.toString("utf8").trim().split(/\s+/)[0];
  const actual = createHash("sha256").update(tarball).digest("hex");
  if (expected !== actual)
    throw new Error(`${name}.tar.gz: SHA-256 mismatch (expected ${expected}, got ${actual})`);
  mkdirSync(dir, { recursive: true });
  // Unpack **inside the destination**, not in the system temp directory.
  // `rename(2)` cannot cross a mount, and a cache on disk with `/tmp` on
  // tmpfs — or a CI runner whose workspace and `/tmp` are different mounts —
  // is an ordinary setup: `EXDEV: cross-device link not permitted` on the
  // very first `usai` of a job. Renaming within one directory is also what
  // makes the install atomic, which is the reason to rename rather than
  // copy, so keeping both properties means putting the work next to the
  // target rather than giving up the rename.
  const work = mkdtempSync(join(dir, ".fetch-"));
  try {
    const tar = spawnSync("tar", ["-xzf", "-", "-C", work], { input: tarball, encoding: "utf8" });
    if (tar.status !== 0) throw new Error(`tar failed: ${tar.error?.message ?? tar.stderr.trim()}`);
    chmodSync(join(work, name, "usai"), 0o755);
    renameSync(join(work, name, "usai"), bin);
  } finally {
    rmSync(work, { recursive: true, force: true });
  }
  // A binary that cannot start on this machine must not be cached, and the
  // loader's message ("version `GLIBC_2.39' not found") is not something a
  // developer should have to translate.
  const probe = spawnSync(bin, ["--version"], { encoding: "utf8" });
  if (probe.status !== 0) {
    rmSync(bin, { force: true });
    const detail = (probe.stderr || probe.error?.message || `exit ${probe.status}`).trim();
    throw new Error(
      `${name} did not start: ${detail}` +
        (/GLIBC/.test(detail)
          ? `\nusai: this release needs glibc ${GLIBC_FLOOR} or newer (Debian 12, Ubuntu 24.04 and later). ` +
            `On an older distribution run the Docker image (ghcr.io/gmedia/usai) or build the binary ` +
            `(\`cargo build --release -p usai-cli\`) and point USAI_BIN at it.`
          : ""),
    );
  }
  return bin;
}

export async function binary() {
  if (process.env.USAI_BIN) return process.env.USAI_BIN;
  if (process.env.USAI_PROBE) return null;
  const found = onPath();
  if (found) return found;
  const triple = target();
  if (!triple) {
    throw new Error(
      `no prebuilt usai binary for ${process.platform}-${process.arch}. ` +
        `Build one with \`cargo build --release -p usai-cli\` and set USAI_BIN, ` +
        `or use the Docker path (docker compose up). See https://github.com/${REPO}#install.`,
    );
  }
  return download(triple);
}

async function main() {
  const bin = await binary();
  if (bin === null) {
    // Probed by another wrapper: not a native binary, so say nothing.
    process.exit(1);
  }
  // The runtime watches this pid: if the wrapper is killed (an IDE task, a
  // supervisor), the server drains and leaves instead of orphaning the port.
  const child = spawn(bin, process.argv.slice(2), {
    stdio: "inherit",
    env: { ...process.env, USAI_PARENT_PID: String(process.pid) },
  });
  // Ctrl-C reaches the child through the process group already; forwarding
  // it too would be the "second signal" that forces the exit. SIGTERM/SIGHUP
  // sent to this wrapper (docker stop, a supervisor) are forwarded once.
  process.on("SIGINT", () => {});
  for (const signal of ["SIGTERM", "SIGHUP"]) process.on(signal, () => child.kill(signal));
  // `pnpm usai run &` then `kill $!` kills pnpm, not this wrapper: when the
  // parent that started us is gone (ppid changes to init), drain the runtime
  // and leave rather than survive as an orphan holding the port.
  const parent = process.ppid;
  const watch = setInterval(() => {
    if (process.ppid !== parent) {
      clearInterval(watch);
      child.kill("SIGTERM");
    }
  }, 1000);
  watch.unref();
  child.on("exit", (code, signal) => {
    // The dynamic loader exits 127 after printing why; say what to do about it.
    if (code === 127 && process.platform === "linux")
      process.stderr.write(
        `usai: ${bin} did not start. The release binaries need glibc ${GLIBC_FLOOR} or newer; ` +
          `on an older distribution use the Docker image or set USAI_BIN to a binary built here.\n`,
      );
    process.exit(code ?? (signal ? 128 + 1 : 1));
  });
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
