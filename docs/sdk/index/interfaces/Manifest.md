[@sakaladev/usai](../../README.md) / [index](../README.md) / Manifest

# Interface: Manifest

What `usai build` writes to `manifest.json`: the application as data —
every workload with its trigger, contracts (JSON Schema) and policies,
every resource with its secret-free configuration, the auth schemes,
the environment contract. Mirrors the runtime's `Manifest` exactly.

## Properties

| Property | Type |
| ------ | ------ |
| <a id="manifestversion"></a> `manifestVersion` | `1` |
| <a id="name"></a> `name` | `string` |
| <a id="description"></a> `description?` | `string` |
| <a id="modules"></a> `modules` | \{ `name`: `string`; `migrations`: `string`[]; `seeders`: `string`[]; \}[] |
| <a id="workloads"></a> `workloads` | `ManifestWorkload`[] |
| <a id="resources"></a> `resources` | \{ `name`: `string`; `kind`: `string`; `module?`: `string`; `config`: `Record`\<`string`, `unknown`\>; `env`: `string`[]; \}[] |
| <a id="auth"></a> `auth` | \{ `name`: `string`; `scheme`: `string`; `header?`: `string`; \}[] |
| <a id="env"></a> `env` | \{ `name`: `string`; `kind`: `string`; `required`: `boolean`; `values`: `string`[]; \}[] |
| <a id="codesha256"></a> `codeSha256` | `string` |
| <a id="builtwith"></a> `builtWith?` | \{ `sdk?`: `string`; `runtime?`: `string`; \} |
| `builtWith.sdk?` | `string` |
| `builtWith.runtime?` | `string` |
