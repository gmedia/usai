#!/usr/bin/env node
// create-usai: scaffold a Usai application from a template.
//
//   pnpm dlx @sakaladev/create-usai <dir> [--template hello]
import { cpSync, existsSync, mkdirSync, readFileSync, readdirSync, statSync, writeFileSync } from "node:fs";
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
export function scaffold(options: ScaffoldOptions): string {
  const template = options.template ?? "hello";
  const source = resolve(here, "..", "templates", template);
  if (!existsSync(source)) throw new Error(`unknown template ${template}`);
  const target = resolve(options.target);
  if (existsSync(target) && readdirSync(target).length > 0) throw new Error(`${target} is not empty`);
  mkdirSync(target, { recursive: true });
  cpSync(source, target, { recursive: true });
  const name = basename(target).replace(/[^a-z0-9-]/gi, "-").toLowerCase();
  const version = options.usaiVersion ?? (JSON.parse(readFileSync(resolve(here, "..", "package.json"), "utf8")) as { version: string }).version;
  walk(target, (file) => {
    const text = readFileSync(file, "utf8");
    const replaced = text.replaceAll("__NAME__", name).replaceAll("__USAI_VERSION__", `^${version}`);
    if (replaced !== text) writeFileSync(file, replaced);
  });
  return target;
}

function main(argv: string[]): void {
  const args = argv.filter((a) => !a.startsWith("--"));
  const templateFlag = argv.find((a) => a.startsWith("--template="))?.slice("--template=".length);
  const dir = args[0];
  if (!dir) {
    console.error("usage: create-usai <dir> [--template=hello]");
    process.exit(2);
  }
  try {
    const target = scaffold(templateFlag ? { target: dir, template: templateFlag } : { target: dir });
    console.log(`created ${target}\n\n  cd ${dir}\n  pnpm install\n  usai dev\n`);
  } catch (error) {
    console.error(`error: ${(error as Error).message}`);
    process.exit(1);
  }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main(process.argv.slice(2));
}
