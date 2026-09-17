#!/usr/bin/env node
// Bundles a Usai application entry into one ES module for the runtime.
// Invoked by `usai build`; not a public API.
//
//   node bundle.mjs <entry> <outfile>
import { build } from "esbuild";

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
    format: "esm",
    platform: "neutral",
    target: "es2022",
    mainFields: ["module", "main"],
    conditions: ["usai", "import", "default"],
    treeShaking: true,
    minify: false,
    sourcemap: false,
    legalComments: "none",
    logLevel: "silent",
    metafile: true,
    define: { "process.env.NODE_ENV": '"production"' },
  });
  process.stdout.write(JSON.stringify({ ok: true, inputs: Object.keys(result.metafile.inputs) }));
} catch (error) {
  const messages = (error && error.errors) ? error.errors.map((e) => ({ text: e.text, location: e.location })) : [{ text: String(error) }];
  process.stdout.write(JSON.stringify({ ok: false, errors: messages }));
  process.exit(1);
}
