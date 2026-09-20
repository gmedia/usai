[@sakaladev/usai](../../README.md) / [index](../README.md) / AppDeclaration

# Interface: AppDeclaration

What [defineApp](../functions/defineApp.md) returns: the application's default export.

## Properties

| Property | Modifier | Type | Description |
| ------ | ------ | ------ | ------ |
| <a id="name"></a> `name` | `readonly` | `string` | - |
| <a id="description"></a> `description?` | `readonly` | `string` | - |
| <a id="headers"></a> `headers?` | `readonly` | `Readonly`\<`Record`\<`string`, `string`\>\> | Response headers set on every application response (`defineApp({ headers })`). |
| <a id="modules"></a> `modules` | `readonly` | readonly [`ModuleDeclaration`](ModuleDeclaration.md)[] | - |
| <a id="workloads"></a> `workloads` | `readonly` | readonly [`Workload`](Workload.md)[] | - |
| <a id="resources"></a> `resources` | `readonly` | readonly [`ResourceDeclaration`](ResourceDeclaration.md)\<`string`, `unknown`\>[] | - |
| <a id="env"></a> `env?` | `readonly` | [`EnvDeclaration`](EnvDeclaration.md)\<`Record`\<`string`, [`EnvField`](EnvField.md)\<`unknown`\>\>\> | - |
