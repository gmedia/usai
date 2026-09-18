# __NAME__

A Usai application.

Three ways to run it — pick one:

```bash
# 1. native binary (https://github.com/gmedia/usai/releases)
pnpm install
usai dev
curl localhost:3000/hello/world

# 2. Docker, nothing installed locally
docker compose up            # runtime + toolchain in the image, your source mounted

# 3. ship it: one image with the runtime and the built artifact, nothing else
docker build -t my-app . && docker run --rm -p 3000:3000 my-app
```

- `src/app.ts` — the application root. Add workloads and resources here or compose modules.
- `usai.config.ts` — project structure only (entry, migrations, seeders).
- `compose.yaml` / `Dockerfile` — the Docker paths above; delete them if you do not use Docker.
- `pnpm-workspace.yaml` — allows esbuild's build script (pnpm blocks dependency scripts by default); npm/yarn users can delete it.
- The `usai` binary: https://github.com/gmedia/usai/releases (or `cargo build --release -p usai-cli` from the repository).
- `usai inspect` — what the runtime understood. `usai config` — effective configuration.
