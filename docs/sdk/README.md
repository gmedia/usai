# @sakaladev/usai v0.0.7

## Modules

| Module | Description |
| ------ | ------ |
| [config](config/README.md) | `@sakaladev/usai/config` — the shape of `usai.config.ts`: project structure only (entry, output directory, migration and seeder globs), declarative. Start with [defineConfig](config/functions/defineConfig.md). |
| [index](index/README.md) | `@sakaladev/usai` — declare work and resources; the runtime gives each its natural lifetime. Public surface for v0 (breaking changes allowed before alpha). Start with [defineApp](index/functions/defineApp.md), then [http](index/variables/http.md), [task](index/functions/task.md), [cron](index/functions/cron.md), [queue](index/variables/queue.md), [postgres](index/functions/postgres.md); every handler receives a [BaseContext](index/interfaces/BaseContext.md). What *your* application declares — every operation, what it leases and hands off, a request panel — is the application reference at `/_usai/docs` on a running `usai dev`. |
| [test](test/README.md) | `@sakaladev/usai/test` — run the same application model tests will meet in production. The harness spawns the `usai` runtime for the project, talks HTTP to the application, and invokes tasks, cron ticks and commands deterministically through the control surface: no wall clock, no external server. Start with [testApp](test/functions/testApp.md). |
