#!/usr/bin/env node
// create-usai: scaffold a Usai application from a template.
//
//   pnpm dlx @sakaladev/create-usai <dir> [--template hello]
import {
  cpSync,
  existsSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  realpathSync,
  renameSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { basename, dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));

export interface ScaffoldOptions {
  target: string;
  template?: string;
  usaiVersion?: string;
}

function walk(dir: string, visit: (file: string) => void): void {
  for (const entry of readdirSync(dir)) {
    const full = join(dir, entry);
    if (statSync(full).isDirectory()) walk(full, visit);
    else visit(full);
  }
}

/** Copies the template and substitutes placeholders. Refuses a non-empty target. */
export function scaffold(options: ScaffoldOptions): { target: string; version: string } {
  const template = options.template ?? "hello";
  const source = resolve(here, "..", "templates", template);
  if (!existsSync(source)) throw new Error(`unknown template ${template}`);
  const target = resolve(options.target);
  if (existsSync(target) && readdirSync(target).length > 0)
    throw new Error(`${target} is not empty`);
  mkdirSync(target, { recursive: true });
  cpSync(source, target, { recursive: true });
  const name = basename(target)
    .replace(/[^a-z0-9-]/gi, "-")
    .toLowerCase();
  // The SDK version to depend on: `sdkVersion` in this package's manifest
  // (kept equal to the published @sakaladev/usai by the release workflow),
  // never this scaffolder's own version — the two have separate cadences.
  const version =
    options.usaiVersion ??
    (
      JSON.parse(readFileSync(resolve(here, "..", "package.json"), "utf8")) as {
        sdkVersion: string;
      }
    ).sdkVersion;
  // compose.yaml runs the dev container as this user so the files it writes
  // into the bind mount are theirs (Linux; Docker Desktop maps ownership
  // itself). Windows has no uid; the image's default user then applies.
  const uid = process.getuid?.() ?? 1000;
  const gid = process.getgid?.() ?? 1000;
  walk(target, (file) => {
    const text = readFileSync(file, "utf8");
    // The image tag is the runtime version, which the release contract keeps
    // equal to the SDK version.
    const replaced = text
      .replaceAll("__NAME__", name)
      .replaceAll("__USAI_VERSION__", `^${version}`)
      .replaceAll("__USAI_IMAGE_TAG__", version)
      .replaceAll("__USAI_UID__", String(uid))
      .replaceAll("__USAI_GID__", String(gid));
    if (replaced !== text) writeFileSync(file, replaced);
  });
  // npm strips `.gitignore` from published packages, so the template ships
  // it as `_gitignore`.
  for (const [from, to] of [
    ["_gitignore", ".gitignore"],
    ["_dockerignore", ".dockerignore"],
    ["_env.example", ".env.example"],
  ] as const) {
    if (existsSync(join(target, from))) renameSync(join(target, from), join(target, to));
  }
  return { target, version };
}

function ownVersion(): string {
  try {
    return (
      JSON.parse(readFileSync(resolve(here, "..", "package.json"), "utf8")) as { version: string }
    ).version;
  } catch {
    return "unknown";
  }
}

/** The templates this package ships, read from disk so the help text cannot
 * fall behind the directory it describes. */
function templates(): string[] {
  try {
    return readdirSync(resolve(here, "..", "templates")).sort();
  } catch {
    return ["hello"];
  }
}

function usage(): string {
  return [
    `create-usai ${ownVersion()} — scaffold a Usai application`,
    "",
    "usage: create-usai <dir> [--template=<name>]",
    "",
    "  <dir>               where to create it (must not exist, or be empty)",
    `  --template=<name>   ${templates().join(", ")} (default: hello)`,
    "  -h, --help          print this and exit",
    "",
    "  npm create @sakaladev/usai@latest my-app",
    "  pnpm dlx @sakaladev/create-usai@latest my-app",
  ].join("\n");
}

function main(argv: string[]): void {
  // Help wins wherever it appears and creates nothing. `--help` used to be
  // filtered out with every other `--` argument, so `create-usai probe
  // --help` scaffolded `probe` and printed no help at all.
  if (argv.includes("--help") || argv.includes("-h")) {
    console.log(usage());
    return;
  }
  // An option this does not know is a typo or a misunderstanding, and
  // ignoring it means scaffolding something other than what was asked for.
  const unknown = argv.filter((a) => a.startsWith("-") && !a.startsWith("--template="));
  if (unknown.length > 0) {
    console.error(`error: unknown option ${unknown.join(", ")}\n\n${usage()}`);
    process.exit(2);
  }
  const args = argv.filter((a) => !a.startsWith("-"));
  const templateFlag = argv.find((a) => a.startsWith("--template="))?.slice("--template=".length);
  const dir = args[0];
  if (!dir) {
    console.error(usage());
    process.exit(2);
  }
  try {
    const { target, version } = scaffold(
      templateFlag ? { target: dir, template: templateFlag } : { target: dir },
    );
    // **Say which SDK this pinned.** `pnpm dlx` serves whatever its own
    // cache holds for a package spec — measured: `pnpm dlx
    // @sakaladev/create-usai` and even `…@latest` scaffolded `^0.0.4` and
    // `^0.0.8` on a machine that had run it before, while `npx -y` on the
    // same machine scaffolded `^0.0.10`. That is the worst shape a
    // first-impression bug can have: right for whoever tests it on a clean
    // machine, wrong for the returning user who tried the last version.
    // A scaffolder cannot fix another tool's cache; it can refuse to be
    // silent about what it wrote.
    console.log(
      `created ${target}\n  @sakaladev/usai ^${version}  (create-usai ${ownVersion()})\n` +
        `\n  cd ${dir}\n  pnpm install\n  pnpm dev\n`,
    );
  } catch (error) {
    console.error(`error: ${(error as Error).message}`);
    process.exit(1);
  }
}

// Run when invoked as the executable. Package managers reach this file
// through a symlink (`node_modules/.bin/create-usai`), so compare real paths:
// `import.meta.url` is already resolved, `process.argv[1]` may not be.
const invokedAs = process.argv[1]
  ? (() => {
      try {
        return realpathSync(process.argv[1]);
      } catch {
        return resolve(process.argv[1]);
      }
    })()
  : "";
if (invokedAs === fileURLToPath(import.meta.url)) {
  main(process.argv.slice(2));
}
