#!/usr/bin/env node
// Bundles a Usai application entry into one ES module for the runtime.
// Invoked by `usai build`; not a public API.
//
//   node bundle.mjs <entry> <outfile>
import { build } from "esbuild";
import { readFileSync } from "node:fs";
import { dirname, relative, resolve, sep } from "node:path";

// Stamped into the bundle so the manifest records which SDK described it.
const sdkVersion = JSON.parse(
  readFileSync(new URL("../package.json", import.meta.url), "utf8"),
).version;

const [entry, outfile] = process.argv.slice(2);

// A module's migrations live next to it, but the bundle has no source
// locations, so `defineModule({ migrations })` had to be written relative to
// the project root — a module could not be moved or copied without editing
// the glob inside it. The build knows each file's path, so it stamps the
// declaring file's directory onto the call; the runtime tries the glob as
// written first and falls back to that directory, which keeps every existing
// application working.
const root = process.cwd();
const stampModuleSource = {
  name: "usai-module-source",
  setup(build) {
    build.onLoad({ filter: /\.(ts|tsx|js|mjs|jsx)$/ }, async (args) => {
      // `node_modules` used to be skipped here, which meant a module
      // *shipped as a package* — the way a shared layer is actually shared —
      // never had its source directory stamped, so its migrations resolved
      // from nowhere: `matches no file`, no `migrations/` in the artifact,
      // and `db migrate` reporting success. A green deploy whose first
      // request said `column does not exist`. The `defineModule` text check
      // below is the cheap filter; the path no longer is.
      const { readFile } = await import("node:fs/promises");
      const text = await readFile(args.path, "utf8");
      if (!text.includes("defineModule")) return null;
      // esbuild resolves symlinks, so a pnpm/yarn link lands on the real
      // directory — which is where the package's SQL is. The path may be
      // outside the project root (a workspace sibling, a store), and
      // `relative` expresses that with `..`, which is exactly what the
      // runtime's module-relative fallback joins against the root.
      const dir = relative(root, dirname(resolve(args.path)))
        .split(sep)
        .join("/");
      // Only the literal-options form; `defineModule(options)` with a
      // variable keeps the root-relative behaviour it always had.
      const stamped = text.replace(
        /\bdefineModule\s*\(\s*\{/g,
        `defineModule({ sourceDir: ${JSON.stringify(dir)},`,
      );
      return stamped === text
        ? null
        : {
            contents: stamped,
            loader: args.path.endsWith("x") ? "tsx" : args.path.endsWith(".ts") ? "ts" : "js",
          };
    });
  },
};
if (!entry || !outfile) {
  console.error("usage: bundle.mjs <entry> <outfile>");
  process.exit(2);
}

try {
  const result = await build({
    entryPoints: [entry],
    outfile,
    plugins: [stampModuleSource],
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
    define: {
      "process.env.NODE_ENV": '"production"',
      __USAI_SDK_VERSION__: JSON.stringify(sdkVersion),
    },
  });
  process.stdout.write(JSON.stringify({ ok: true, inputs: Object.keys(result.metafile.inputs) }));
} catch (error) {
  const messages =
    error && error.errors
      ? error.errors.map((e) => ({ text: e.text, location: e.location }))
      : [{ text: String(error) }];
  process.stdout.write(JSON.stringify({ ok: false, errors: messages }));
  process.exit(1);
}
