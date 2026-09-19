[@sakaladev/usai](../../README.md) / [index](../README.md) / resolveEnv

# Function: resolveEnv()

```ts
function resolveEnv<S extends Record<string, EnvField<unknown>>>(decl: EnvDeclaration<S>, raw: Record<string, string | undefined>): EnvValues<S>;
```

Resolve declared values from a raw map (what the host does at
activation). Throws on the first violation, naming the variable.

## Type Parameters

| Type Parameter |
| ------ |
| `S` *extends* `Record`\<`string`, [`EnvField`](../interfaces/EnvField.md)\<`unknown`\>\> |

## Parameters

| Parameter | Type |
| ------ | ------ |
| `decl` | [`EnvDeclaration`](../interfaces/EnvDeclaration.md)\<`S`\> |
| `raw` | `Record`\<`string`, `string` \| `undefined`\> |

## Returns

[`EnvValues`](../type-aliases/EnvValues.md)\<`S`\>
