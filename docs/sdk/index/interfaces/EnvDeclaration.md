[@sakaladev/usai](../../README.md) / [index](../README.md) / EnvDeclaration

# Interface: EnvDeclaration\<S *extends* `Record`\<`string`, [`EnvField`](EnvField.md)\<`unknown`\>\>\>

The application's environment contract (`defineApp({ env })`).

## Type Parameters

| Type Parameter |
| ------ |
| `S` *extends* `Record`\<`string`, [`EnvField`](EnvField.md)\<`unknown`\>\> |

## Properties

| Property | Modifier | Type |
| ------ | ------ | ------ |
| <a id="__usai"></a> `__usai` | `readonly` | `"env"` |
| <a id="fields"></a> `fields` | `readonly` | `S` |
