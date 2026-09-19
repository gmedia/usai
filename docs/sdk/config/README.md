[@sakaladev/usai](../README.md) / config

# config

`@sakaladev/usai/config` — the shape of `usai.config.ts`: project
structure only (entry, output directory, migration and seeder globs),
declarative. Start with [defineConfig](functions/defineConfig.md).

## Configuration

| Name | Description |
| ------ | ------ |
| [DatabaseConfig](interfaces/DatabaseConfig.md) | Where the SQL migrations and seeders live, as globs from the project root. |
| [UsaiConfig](interfaces/UsaiConfig.md) | `usai.config.ts`: project structure only (entry, output, database globs). Declarative — the file is evaluated in a capability-less world, so it cannot read the environment or the file system. |
| [defineConfig](functions/defineConfig.md) | The default export of `usai.config.ts`. |
