# __NAME__

A Usai application.

Three ways to run it — pick one:

```bash
# 1. native — `pnpm dev` runs the `usai` binary at this project's SDK version
#    (fetched once from the GitHub release, SHA-256 verified; a `usai` already
#    on your PATH at that version is used instead)
pnpm install
pnpm dev
curl localhost:3000/hello/world

# 2. Docker, nothing installed locally
docker compose up            # runtime + toolchain in the image, your source mounted
#    (to scaffold without Node at all: docker run --rm -v "$PWD:/app" -w /app --entrypoint sh \
#     sakaladev/usai:<v>-dev -c 'pnpm dlx @sakaladev/create-usai my-app')

# 3. ship it: one image with the runtime and the built artifact, nothing else
docker build -t my-app . && docker run --rm -p 3000:3000 my-app
#    migrations ride in the artifact: docker run --rm -e DATABASE_URL=… my-app db migrate --artifact /app/.usai/build
```

- `test/hello.test.ts` — a test through the real runtime (`pnpm test` finds `src|test|tests/**/*.test.ts`).
- `src/app.ts` — the application root. Add workloads and resources here or compose modules.
- `usai.config.ts` — project structure only (entry, migrations, seeders).
- `compose.yaml` / `Dockerfile` — the Docker paths above; delete them if you do not use Docker.
- `pnpm-workspace.yaml` — allows esbuild's build script (pnpm blocks dependency scripts by default); npm/yarn users can delete it.
- The `usai` binary: `pnpm usai …` runs it through the SDK (cache `~/.cache/usai/<version>`); or install it from https://github.com/gmedia/usai/releases (or `cargo build --release -p usai-cli` from the repository) and call `usai` directly.
- `pnpm usai keygen` → `pnpm usai build --sign usai-signing.key` → run with `--require-signature <public key>`; `*.key` is git-ignored.
- `pnpm usai inspect` — what the runtime understood. `pnpm usai config` — effective configuration. `pnpm test` — the project's tests.
- While `pnpm dev` runs, http://localhost:3000/_usai/docs is the application reference: every operation and workload, what it does to the system, and a request panel. `pnpm usai generate openapi --public --out openapi.json` writes the consumer contract (no runtime facts) for API consumers.
