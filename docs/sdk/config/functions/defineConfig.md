[@sakaladev/usai](../../README.md) / [config](../README.md) / defineConfig

# Function: defineConfig()

```ts
function defineConfig(config: UsaiConfig): UsaiConfig & {
  __usai: "config";
};
```

The default export of `usai.config.ts`.

## Parameters

| Parameter | Type |
| ------ | ------ |
| `config` | [`UsaiConfig`](../interfaces/UsaiConfig.md) |

## Returns

[`UsaiConfig`](../interfaces/UsaiConfig.md) & \{
  `__usai`: `"config"`;
\}

## Example

```ts
import { defineConfig } from "@sakaladev/usai/config";
export default defineConfig({ app: "./src/app.ts", database: { migrations: { include: ["./migrations/*.sql"] } } });
```
