[@sakaladev/usai](../../README.md) / [index](../README.md) / describe

# Function: describe()

```ts
function describe(app: AppDeclaration): Manifest;
```

Turn an [AppDeclaration](../interfaces/AppDeclaration.md) into its [Manifest](../interfaces/Manifest.md). The build
phase calls it inside a capability-less world; call it yourself to
assert on an application's shape in a unit test. Throws on a duplicate
workload, a conflicting resource redeclaration, or a hole in a list.

## Parameters

| Parameter | Type |
| ------ | ------ |
| `app` | [`AppDeclaration`](../interfaces/AppDeclaration.md) |

## Returns

[`Manifest`](../interfaces/Manifest.md)
