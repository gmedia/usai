[@sakaladev/usai](../../README.md) / [index](../README.md) / DefineModuleOptions

# Interface: DefineModuleOptions

Options for [defineModule](../functions/defineModule.md).

## Properties

| Property | Type | Description |
| ------ | ------ | ------ |
| <a id="name"></a> `name` | `string` | Module name: groups operations in the reference and OpenAPI tags. |
| <a id="workloads"></a> `workloads?` | [`Workload`](Workload.md)[] | - |
| <a id="resources"></a> `resources?` | [`ResourceDeclaration`](ResourceDeclaration.md)\<`string`, `unknown`\>[] | Resources this module declares; the same resource may be declared by several modules with identical configuration. |
| <a id="migrations"></a> `migrations?` | `string` \| `string`[] | Glob(s) for this module's SQL migrations, relative to the project root (e.g. `./src/billing/migrations/*.sql`). The bundle carries no source locations, so module-relative paths are not supported in v0. |
| <a id="seeders"></a> `seeders?` | `string` \| `string`[] | Glob(s) for this module's seeder files, relative to the project root. |
