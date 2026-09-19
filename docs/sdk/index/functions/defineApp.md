[@sakaladev/usai](../../README.md) / [index](../README.md) / defineApp

# Function: defineApp()

```ts
function defineApp(options?: DefineAppOptions): AppDeclaration;
```

The application root: the default export of the entry file. Everything
the runtime will ever run is reachable from here — modules, top-level
workloads, resources and the environment contract — which is why
`usai inspect`, `usai graph`, the OpenAPI document and the reference
page all read this one object. Declaring a workload twice, or leaving
a hole in a list (a `const` used before it ran), is a build error that
names the slot.

## Parameters

| Parameter | Type |
| ------ | ------ |
| `options` | [`DefineAppOptions`](../interfaces/DefineAppOptions.md) |

## Returns

[`AppDeclaration`](../interfaces/AppDeclaration.md)

## Example

```ts
export default defineApp({
  name: "invoicing",
  description: "Multi-tenant invoicing with webhook delivery.",
  modules: [authModule, invoices, webhooks],
  env: env({ DATABASE_URL: env.url(), SESSION_TTL_HOURS: env.optional(env.int()) }),
});
```
