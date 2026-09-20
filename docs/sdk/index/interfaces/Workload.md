[@sakaladev/usai](../../README.md) / [index](../README.md) / Workload

# Interface: Workload

A declared unit of work, whatever its kind — what every `http.*`,
`task`, `cron`, `command`, `service`, `queue.consume`, `socket` and
`http.stream` call returns and what `defineApp`/`defineModule` list.
Plain data: the build phase reads it into the manifest, the runtime
routes to it, `inspect`/`graph`/the reference page render it.

## Properties

| Property | Modifier | Type | Description |
| ------ | ------ | ------ | ------ |
| <a id="kind"></a> `kind` | `readonly` | \| `"http"` \| `"task"` \| `"cron"` \| `"command"` \| `"service"` \| `"queue"` \| `"socket"` \| `"stream"` | - |
| <a id="name"></a> `name` | `readonly` | `string` | - |
| <a id="summary"></a> `summary?` | `readonly` | `string` | One line about the workload, for the reference and the OpenAPI `summary`. |
| <a id="description"></a> `description?` | `readonly` | `string` | A paragraph about the workload, for the reference and the OpenAPI `description`. |
| <a id="trigger"></a> `trigger` | `readonly` | `Record`\<`string`, `unknown`\> | Kind-specific facts (method and path, schedule, topic, …), as the manifest carries them. |
| <a id="contracts"></a> `contracts` | `readonly` | \{ `params?`: [`AnySchema`](../type-aliases/AnySchema.md); `query?`: [`AnySchema`](../type-aliases/AnySchema.md); `headers?`: [`AnySchema`](../type-aliases/AnySchema.md); `body?`: [`AnySchema`](../type-aliases/AnySchema.md); `input?`: [`AnySchema`](../type-aliases/AnySchema.md); `message?`: [`AnySchema`](../type-aliases/AnySchema.md); `response?`: `Record`\<`number`, [`AnySchema`](../type-aliases/AnySchema.md)\>; \} | - |
| `contracts.params?` | `public` | [`AnySchema`](../type-aliases/AnySchema.md) | - |
| `contracts.query?` | `public` | [`AnySchema`](../type-aliases/AnySchema.md) | - |
| `contracts.headers?` | `public` | [`AnySchema`](../type-aliases/AnySchema.md) | - |
| `contracts.body?` | `public` | [`AnySchema`](../type-aliases/AnySchema.md) | - |
| `contracts.input?` | `public` | [`AnySchema`](../type-aliases/AnySchema.md) | - |
| `contracts.message?` | `public` | [`AnySchema`](../type-aliases/AnySchema.md) | - |
| `contracts.response?` | `public` | `Record`\<`number`, [`AnySchema`](../type-aliases/AnySchema.md)\> | - |
| <a id="errors"></a> `errors` | `readonly` | [`DeclaredError`](DeclaredError.md)[] | - |
| <a id="responseheaders"></a> `responseHeaders?` | `readonly` | [`ResponseHeaderDocs`](../type-aliases/ResponseHeaderDocs.md) | Documented response headers per status (HTTP, raw and stream workloads). |
| <a id="operationid"></a> `operationId?` | `readonly` | `string` | The chosen OpenAPI `operationId`, when the declaration set one. |
| <a id="auth"></a> `auth?` | `readonly` | [`AuthDeclaration`](AuthDeclaration.md)\<`unknown`\> | - |
| <a id="resources"></a> `resources` | `readonly` | [`ResourceDeclaration`](ResourceDeclaration.md)\<`string`, `unknown`\>[] | - |
| <a id="dispatches"></a> `dispatches` | `readonly` | `Workload`[] | - |
| <a id="publishes"></a> `publishes` | `readonly` | `string`[] | Queue topics this workload publishes to (`publishes(...)`). |
| <a id="policies"></a> `policies` | `readonly` | [`WorkloadPolicies`](WorkloadPolicies.md) | - |
