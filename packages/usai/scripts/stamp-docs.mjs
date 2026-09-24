// The generated reference carries the package's version number and nothing
// else, so `v0.0.9` on a checkout of `main` means "the sources *after*
// 0.0.9" — and a reader who installs 0.0.9 from the registry finds a
// surface the pages describe and the package does not have (measured on
// `TypedWorkload`, which a round hit in its first hour). The pages now say
// which they are.
import { readFile, writeFile } from "node:fs/promises";
import { join } from "node:path";

const readme = join(import.meta.dirname, "../../../docs/sdk/README.md");
const banner = [
  "",
  "> **These pages describe `main`, not the last release.** They are generated",
  "> from the SDK sources in this repository, so they can document a surface a",
  "> published package does not have yet; `CHANGELOG.md` says what is still",
  "> unreleased. For the reference that matches what you installed, read the",
  "> `.d.ts` in your own `node_modules/@sakaladev/usai`.",
  "",
].join("\n");

const text = await readFile(readme, "utf8");
if (text.includes("These pages describe `main`")) process.exit(0);
const lines = text.split("\n");
const title = lines.findIndex((l) => l.startsWith("# "));
if (title === -1) {
  console.error("docs/sdk/README.md has no title to stamp");
  process.exit(1);
}
lines.splice(title + 1, 0, banner);
await writeFile(readme, lines.join("\n"));
