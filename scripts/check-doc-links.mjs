#!/usr/bin/env node
// Every relative link in `docs/sdk` must resolve: the file has to exist, and
// an anchor has to be one the page actually defines.
//
// TypeDoc writes a link for `{@link x.y}` whether or not it produced a page
// section for `y`, and `env.group` is a function attached to `env` after the
// fact — so the link came out as `env.md#group` against a page with no such
// anchor. Nothing here noticed; the site that mirrors these pages ran a link
// checker and its scheduled sync failed instead, six hours after a release.
// This is that check, upstream, where the link is written.
import { readFileSync } from "node:fs";
import { readdir } from "node:fs/promises";
import { dirname, join, relative, resolve } from "node:path";

const root = resolve(process.argv[2] ?? "docs/sdk");

async function markdown(dir) {
  const out = [];
  for (const entry of await readdir(dir, { withFileTypes: true })) {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) out.push(...(await markdown(path)));
    else if (entry.name.endsWith(".md")) out.push(path);
  }
  return out;
}

/** The anchors a page defines: explicit `<a id>` and heading slugs. */
function anchorsOf(text) {
  const ids = new Set();
  for (const m of text.matchAll(/<a id="([^"]+)"><\/a>/g)) ids.add(m[1].toLowerCase());
  for (const m of text.matchAll(/^#{1,6}\s+(.+?)\s*$/gm)) {
    ids.add(
      m[1]
        .toLowerCase()
        .replace(/[^a-z0-9 -]/g, "")
        .trim()
        .replace(/\s+/g, "-"),
    );
  }
  return ids;
}

const files = await markdown(root);
const anchors = new Map(files.map((f) => [resolve(f), anchorsOf(readFileSync(f, "utf8"))]));
const broken = [];
for (const file of files) {
  const text = readFileSync(file, "utf8");
  for (const m of text.matchAll(/\]\((?!https?:|mailto:)([^)\s]+)\)/g)) {
    const target = decodeURIComponent(m[1]);
    const [path, fragment] = target.split("#");
    const dest = path ? resolve(dirname(file), path) : resolve(file);
    if (!anchors.has(dest)) broken.push(`${relative(root, file)} -> ${target} (no such page)`);
    else if (fragment && !anchors.get(dest).has(fragment.toLowerCase()))
      broken.push(`${relative(root, file)} -> ${target} (the page has no such section)`);
  }
}

if (broken.length > 0) {
  console.error(`docs/sdk has ${broken.length} broken link(s):`);
  for (const line of broken) console.error(`  ${line}`);
  console.error(
    "\nA `{@link a.b}` to a property attached to a function (`env.group`) has no page\n" +
      "section to point at: write it as `a.b` in backticks instead.",
  );
  process.exit(1);
}
console.log(`docs/sdk: ${files.length} pages, every link resolves`);
