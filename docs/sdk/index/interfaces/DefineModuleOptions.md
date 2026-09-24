[@sakaladev/usai](../../README.md) / [index](../README.md) / DefineModuleOptions

# Interface: DefineModuleOptions

Options for [defineModule](../functions/defineModule.md).

## Properties

| Property | Type | Description |
| ------ | ------ | ------ |
| <a id="name"></a> `name` | `string` | Module name: groups operations in the reference and OpenAPI tags. |
| <a id="workloads"></a> `workloads?` | [`Workload`](Workload.md)[] | - |
| <a id="resources"></a> `resources?` | [`ResourceDeclaration`](ResourceDeclaration.md)\<`string`, `unknown`\>[] | Resources this module declares; the same resource may be declared by several modules with identical configuration. |
| <a id="migrations"></a> `migrations?` | `string` \| `string`[] | Glob(s) for this module's SQL migrations. **Write them relative to the module's own file** (`./migrations/*.sql` in `src/billing/module.ts`); a glob is also tried as written from the project root, so `./src/billing/migrations/*.sql` — the only form earlier versions accepted — keeps working. |
| <a id="seeders"></a> `seeders?` | `string` \| `string`[] | Glob(s) for this module's seeder files, relative to the module's own file or to the project root (the same rule as `migrations`). |
| <a id="env"></a> `env?` | [`EnvDeclaration`](EnvDeclaration.md)\<`Record`\<`string`, [`EnvField`](EnvField.md)\<`unknown`\>\>\> | What this module needs from the environment, declared the same way an application declares its own (`env({ BILLING_TAX_RATE: env.string() })`). It is merged into the application's contract, so a missing or unparsable value **fails activation** rather than surfacing as a 500 on the first request that reaches the module — which is what happened when a module had no way to say this and every consumer had to mirror the variable into its own `env({})` by hand, with nothing checking that they had. A key declared by two modules must be declared identically; the application's own declaration of the same key wins, so a consumer can still widen or rename nothing but can tighten what it needs. |
| <a id="sourcedir"></a> `sourceDir?` | `string` | Filled in by the build, not by you: the directory the `defineModule` call was written in, relative to the project root. It is what lets a module's globs be written relative to the module itself — the runtime tries a glob as written first and falls back to this directory. |
