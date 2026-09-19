[@sakaladev/usai](../../README.md) / [test](../README.md) / TestApp

# Interface: TestApp

A running application under test; see [testApp](../functions/testApp.md).

## Properties

| Property | Modifier | Type | Description |
| ------ | ------ | ------ | ------ |
| <a id="url"></a> `url` | `readonly` | `string` | Base URL of the application listener. |
| <a id="controlurl"></a> `controlUrl` | `readonly` | `string` | Base URL of the control surface. |
| <a id="http"></a> `http` | `readonly` | \{ `get`: `Promise`\<[`TestResponse`](TestResponse.md)\>; `post`: `Promise`\<[`TestResponse`](TestResponse.md)\>; `put`: `Promise`\<[`TestResponse`](TestResponse.md)\>; `patch`: `Promise`\<[`TestResponse`](TestResponse.md)\>; `delete`: `Promise`\<[`TestResponse`](TestResponse.md)\>; `request`: `Promise`\<[`TestResponse`](TestResponse.md)\>; \} | HTTP requests to the application, with JSON in and out. |
| `http.get` | `public` | `Promise`\<[`TestResponse`](TestResponse.md)\> | - |
| `http.post` | `public` | `Promise`\<[`TestResponse`](TestResponse.md)\> | - |
| `http.put` | `public` | `Promise`\<[`TestResponse`](TestResponse.md)\> | - |
| `http.patch` | `public` | `Promise`\<[`TestResponse`](TestResponse.md)\> | - |
| `http.delete` | `public` | `Promise`\<[`TestResponse`](TestResponse.md)\> | - |
| `http.request` | `public` | `Promise`\<[`TestResponse`](TestResponse.md)\> | - |

## Methods

### task()

```ts
task(name: string): {
  invoke: Promise<WorkOutcome<T>>;
};
```

Run a task once in a fresh world and get its [WorkOutcome](WorkOutcome.md).

#### Parameters

| Parameter | Type |
| ------ | ------ |
| `name` | `string` |

#### Returns

```ts
{
  invoke: Promise<WorkOutcome<T>>;
}
```

| Name | Type |
| ------ | ------ |
| `invoke()` | (`input?`: `unknown`) => `Promise`\<[`WorkOutcome`](WorkOutcome.md)\<`T`\>\> |

***

### cron()

```ts
cron(name: string): {
  run: Promise<WorkOutcome<T>>;
};
```

Run one cron tick, without the clock.

#### Parameters

| Parameter | Type |
| ------ | ------ |
| `name` | `string` |

#### Returns

```ts
{
  run: Promise<WorkOutcome<T>>;
}
```

| Name | Type |
| ------ | ------ |
| `run()` | () => `Promise`\<[`WorkOutcome`](WorkOutcome.md)\<`T`\>\> |

***

### command()

```ts
command(name: string): {
  run: Promise<WorkOutcome<T>>;
};
```

Run a command with arguments.

#### Parameters

| Parameter | Type |
| ------ | ------ |
| `name` | `string` |

#### Returns

```ts
{
  run: Promise<WorkOutcome<T>>;
}
```

| Name | Type |
| ------ | ------ |
| `run()` | (`args?`: `string`[]) => `Promise`\<[`WorkOutcome`](WorkOutcome.md)\<`T`\>\> |

***

### status()

```ts
status(): Promise<Record<string, unknown>>;
```

Runtime status JSON (`/status` on the control surface).

#### Returns

`Promise`\<`Record`\<`string`, `unknown`\>\>

***

### close()

```ts
close(): Promise<void>;
```

Stop the runtime (drains, then exits). Always call it, in `after`.

#### Returns

`Promise`\<`void`\>
