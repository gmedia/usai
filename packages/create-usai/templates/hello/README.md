# __NAME__

A Usai application.

```bash
pnpm install
usai dev
curl localhost:3000/hello/world
```

- `src/app.ts` — the application root. Add workloads and resources here or compose modules.
- `usai.config.ts` — project structure only (entry, migrations, seeders).
- `pnpm-workspace.yaml` — allows esbuild's build script (pnpm blocks dependency scripts by default); npm/yarn users can delete it.
- The `usai` binary: https://github.com/gmedia/usai/releases (or `cargo build --release -p usai-cli` from the repository).
- `usai inspect` — what the runtime understood. `usai config` — effective configuration.
