[@sakaladev/usai](../../README.md) / [index](../README.md) / defineModule

# Function: defineModule()

```ts
function defineModule(options: DefineModuleOptions): ModuleDeclaration;
```

Group workloads, resources, migrations and seeders under a name. A
module is organisation, not isolation: its workloads run like any other,
and a resource it declares is shared with every module that declares the
same one. Modules are the unit that owns SQL migrations.

## Parameters

| Parameter | Type |
| ------ | ------ |
| `options` | [`DefineModuleOptions`](../interfaces/DefineModuleOptions.md) |

## Returns

[`ModuleDeclaration`](../interfaces/ModuleDeclaration.md)

## Example

```ts
export const invoices = defineModule({
  name: "invoices",
  workloads: [list, get, create, issue, pay, markOverdue],
  resources: [db],
  migrations: "./src/invoices/migrations/*.sql",
});
```
