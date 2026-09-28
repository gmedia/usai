[@sakaladev/usai](../../README.md) / [index](../README.md) / EnvField

# Interface: EnvField\<T\>

One declared variable: kind, whether it is required, and its parser.

## Type Parameters

| Type Parameter |
| ------ |
| `T` |

## Properties

| Property | Modifier | Type | Description |
| ------ | ------ | ------ | ------ |
| <a id="__usai"></a> `__usai` | `readonly` | `"env-field"` | - |
| <a id="kind"></a> `kind` | `readonly` | [`EnvKind`](../type-aliases/EnvKind.md) | - |
| <a id="required"></a> `required` | `readonly` | `boolean` | - |
| <a id="values"></a> `values?` | `readonly` | readonly `string`[] | - |
| <a id="items"></a> `items?` | `readonly` | [`EnvKind`](../type-aliases/EnvKind.md) | `list` only: the kind each item must be, so the **host** can check them at activation. A parser that only runs in a world cannot fail a deployment, which is the whole point of declaring the variable. |
| <a id="separator"></a> `separator?` | `readonly` | `string` | `list` only: what separates the items (default `,`). |
| <a id="group"></a> `group?` | `readonly` | `string` | The group this field belongs to (`env.group`). Every field of one group must be set together or not at all, checked by the **host** at activation. It lives on the field rather than on the declaration because a field map is routinely spread into a larger one (`env({ ...authEnv.fields })`), and a rule attached to the declaration would vanish there without a word. |
| <a id="parse"></a> `parse` | `readonly` | (`raw`: `string`) => `T` | - |
| <a id="_type"></a> `_type?` | `readonly` | `T` | - |
