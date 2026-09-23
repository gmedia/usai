[@sakaladev/usai](../../README.md) / [index](../README.md) / ModuleDeclaration

# Interface: ModuleDeclaration

What [defineModule](../functions/defineModule.md) returns.

## Properties

| Property | Modifier | Type | Description |
| ------ | ------ | ------ | ------ |
| <a id="name"></a> `name` | `readonly` | `string` | - |
| <a id="workloads"></a> `workloads` | `readonly` | readonly [`Workload`](Workload.md)[] | - |
| <a id="resources"></a> `resources` | `readonly` | readonly [`ResourceDeclaration`](ResourceDeclaration.md)\<`string`, `unknown`\>[] | - |
| <a id="migrations"></a> `migrations` | `readonly` | readonly `string`[] | - |
| <a id="seeders"></a> `seeders` | `readonly` | readonly `string`[] | - |
| <a id="sourcedir"></a> `sourceDir?` | `readonly` | `string` | The directory the declaration was written in (stamped by the build). |
