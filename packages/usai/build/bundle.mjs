#!/usr/bin/env node
// Bundles a Usai application entry into one ES module for the runtime.
// Invoked by `usai build`; not a public API.
//
//   node bundle.mjs <entry> <outfile>
import { build } from "esbuild";
import { readFileSync } from "node:fs";

// Stamped into the bundle so the manifest records which SDK described it.
const sdkVersion = JSON.parse(readFileSync(new URL("../package.json", import.meta.url), "utf8")).version;

const [entry, outfile] = process.argv.slice(2);
if (!entry || !outfile) {
  console.error("usage: bundle.mjs <entry> <outfile>");
  process.exit(2);
}

try {
  const result = await build({
    entryPoints: [entry],
    outfile,
    bundle: true,
    format: "iife",
    globalName: "__usai_app_ns",
    // Evaluated as a module (native engine) `var` is module-scoped; publish
    // the namespace explicitly so both substrates find it.
    footer: { js: "globalThis.__usai_app_ns = __usai_app_ns;" },
    platform: "neutral",
    target: "es2022",
    mainFields: ["module", "main"],
    conditions: ["usai", "import", "default"],
    treeShaking: true,
    minify: false,
    // An external map next to the bundle: the runtime maps `usai:app:L:C`
    // frames in error output to source positions. Sources are paths only.
    sourcemap: "external",
    sourcesContent: false,
    legalComments: "none",
    logLevel: "silent",
    metafile: true,
    define: { "process.env.NODE_ENV": '"production"', __USAI_SDK_VERSION__: JSON.stringify(sdkVersion) },
  });
  process.stdout.write(JSON.stringify({ ok: true, inputs: Object.keys(result.metafile.inputs) }));
} catch (error) {
  const messages = (error && error.errors) ? error.errors.map((e) => ({ text: e.text, location: e.location })) : [{ text: String(error) }];
  process.stdout.write(JSON.stringify({ ok: false, errors: messages }));
  process.exit(1);
}
