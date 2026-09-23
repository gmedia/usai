# Cutting a release

One tag produces everything: the binaries on the GitHub release, the npm
packages, and the Docker images. `.github/workflows/release.yml` does the
work and **refuses to publish anything whose version does not equal the
tag**, so the whole procedure is: make the three versions agree, make the
tree green, tag, push, watch.

Releases are a maintainer's decision, not automation's: the runtime is
alpha, and what a version number promises (`SUPPORTED.md` → Versioning) is
a promise to whoever deploys it.

## 1. The three versions

The workflow checks these against the tag and fails the job if any differs:

| Where | What | Checked in |
|---|---|---|
| `Cargo.toml` (workspace `version`) | the runtime and the CLI (`version.workspace = true`) | the `binaries` job |
| `packages/usai/package.json` `version` | the SDK on npm | the `npm` job |
| `packages/create-usai/package.json` `sdkVersion` | what a scaffolded project depends on | the `npm` job (must equal the SDK version) |

`packages/create-usai`'s own `version` has a **separate cadence** — bump it
only when the scaffolder itself changed; the `npm` job skips a package whose
version is already on the registry, so a release that does not touch the
scaffolder simply does not republish it.

```bash
v=0.0.6
sed -i "0,/^version = .*/s//version = \"$v\"/" Cargo.toml   # check the diff, it is the one under [workspace.package]
node -e 'const f="packages/usai/package.json",p=require("./"+f);p.version=process.argv[1];require("fs").writeFileSync(f,JSON.stringify(p,null,2)+"\n")' $v
node -e 'const f="packages/create-usai/package.json",p=require("./"+f);p.sdkVersion=process.argv[1];require("fs").writeFileSync(f,JSON.stringify(p,null,2)+"\n")' $v
cargo update -w        # Cargo.lock carries the workspace version
pnpm install --lockfile-only
```

## 2. The gate

```bash
make check                 # fmt, clippy -D warnings, docs-check, cargo test, pnpm test
USAI_ENGINE=quickjs cargo test -p usai-runtime --test lifecycle --test http   # the reference engine
cargo test -p usai-runtime --test postgres    # needs a PostgreSQL (USAI_TEST_DATABASE_URL)
```

`make docs-check` fails when `docs/sdk` is stale — regenerate with
`make docs` and commit it in the same change as the code it documents. The
version bump itself changes one line of `docs/sdk/README.md`, and the check
reads `git status`, so it only goes green **after** the release commit:
regenerate, commit, then run the gate on the committed tree.

The release notes are `CHANGELOG.md`: its top section must be the version
being cut (rename `— Unreleased` to the date), and its **Compatibility**
section is what an operator upgrading reads. Anything an existing
deployment must do before or after the upgrade belongs there, not only in a
runbook.

## 3. Tag and push

```bash
git commit -am "release $v"
git tag -a "v$v" -m "$(awk '/^> /{f=1} f&&/^$/{exit} f' CHANGELOG.md | sed 's/^> //')"  # the newest drafted tag message
git push origin main "v$v"
```

The tag is what triggers the workflow; pushing `main` first keeps the
release commit from being orphaned if the tag push is rejected.

## 4. What the workflow does, in order

1. **binaries** (x86_64-linux, aarch64-linux, aarch64-macOS): `cargo build
   --profile dist -p usai-cli`, tarballs + SHA-256 on the GitHub release,
   with generated release notes. The Linux binaries are kept as artifacts
   for the images, so **the image's `usai` and the tarball's `usai` are the
   same bits**. The Linux binaries are compiled *inside* `rust:1.98-bookworm`,
   not on the runner: a binary carries the glibc it was built against, and the
   runner's is newer than the floor in `SUPPORTED.md` (this is what broke
   0.0.6 — see the CHANGELOG). The job then checks the highest `GLIBC_*`
   symbol the binary asks for and starts it in `debian:bookworm-slim` before
   uploading, so the floor cannot silently move again.
2. **npm**: builds the workspace, checks the versions, publishes
   `@sakaladev/usai` and `@sakaladev/create-usai` with Trusted Publishing
   (OIDC — never a long-lived token, never bypass 2FA) and `--provenance`
   on a public repository. A version already on the registry is skipped.
3. **docker** (one native runner per architecture): the runtime image
   (`docker/runtime.Dockerfile`) and the dev image (`docker/dev.Dockerfile`)
   from that architecture's binary, pushed by digest.
4. **docker-manifest**: joins the architectures into `X.Y.Z`, `X.Y`, `X`,
   `latest` and `X.Y.Z-dev`, `latest-dev` — on GHCR always, and on Docker
   Hub (`sakaladev/usai`, the canonical address) when the Docker Hub
   secrets exist.
5. **container-smoke**: scaffolds a project with the **published**
   `create-usai`, builds the app image from the template Dockerfile, runs it
   read-only as non-root, expects a 200, no toolchain in the runtime image,
   and a SIGTERM that drains and exits 0.

## 5. Watch, then verify from the outside

```bash
gh run watch                                   # the release workflow
gh release view "v$v"                          # tarballs + checksums present
npm view @sakaladev/usai version               # the registry agrees
docker run --rm sakaladev/usai:$v --version    # the published image
pnpm dlx @sakaladev/create-usai@latest demo && cd demo && pnpm install && pnpm dev   # the pure-npm path
```

`usai_build_info{version="X.Y.Z"}` is the in-band rollout check — it is the
only thing that tells a scraper which version answered, so a deployment
watching a roll should graph it per instance.

The last one is the path a new user takes: it fetches the release binary
through the launcher (`USAI_RELEASE_BASE` for a mirror) and must serve
`/hello/world`.

## If something fails

- **A version check fails** — nothing was published: fix the version, delete
  the tag (`git push --delete origin vX.Y.Z`), tag again. Deleting a tag is
  safe only while nothing consumed it; if the npm job already published,
  **do not reuse the version** (npm forbids republishing) — cut the next
  patch instead.
- **npm published but the images failed** — rerun the failed jobs (`gh run
  rerun --failed`); the npm job is idempotent (it skips what exists).
- **The images are wrong** — images are immutable by digest; push a new
  patch version rather than moving a tag. `latest` follows the newest tag,
  so a broken `latest` is fixed by releasing again.
- **A release must be withdrawn** — yank the npm version
  (`npm deprecate @sakaladev/usai@X.Y.Z "…"`; unpublishing is only possible
  within 72 hours and breaks anyone who installed it), mark the GitHub
  release as a pre-release, and say so in `CHANGELOG.md`. Docker tags stay:
  point people at the previous one.
