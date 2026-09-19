[@sakaladev/usai](../../README.md) / [config](../README.md) / UsaiConfig

# Interface: UsaiConfig

`usai.config.ts`: project structure only (entry, output, database
globs). Declarative — the file is evaluated in a capability-less world,
so it cannot read the environment or the file system.

## Properties

| Property | Type | Description |
| ------ | ------ | ------ |
| <a id="app"></a> `app?` | `string` | Application entry. Default `./src/app.ts`. |
| <a id="outdir"></a> `outDir?` | `string` | Build output directory. Default `.usai/build`. |
| <a id="database"></a> `database?` | [`DatabaseConfig`](DatabaseConfig.md) | - |
