[@sakaladev/usai](../../README.md) / [index](../README.md) / TypedWorkload

# Interface: TypedWorkload\<Out = `unknown`\>

A [Workload](Workload.md) that remembers what its handler returns, so
`ctx.tasks.invoke(thatTask)` is typed instead of `unknown`. The parameter
is a phantom: nothing carries it at runtime, and a `TypedWorkload` is a
`Workload` everywhere one is expected.

## Extends

- [`Workload`](Workload.md)

## Type Parameters

| Type Parameter | Default type |
| ------ | ------ |
| `Out` | `unknown` |

## Properties

| Property | Modifier | Type | Description | Inherited from |
| ------ | ------ | ------ | ------ | ------ |
| <a id="kind"></a> `kind` | `readonly` | \| `"http"` \| `"task"` \| `"cron"` \| `"command"` \| `"service"` \| `"queue"` \| `"socket"` \| `"stream"` | - | [`Workload`](Workload.md).[`kind`](Workload.md#kind) |
| <a id="name"></a> `name` | `readonly` | `string` | - | [`Workload`](Workload.md).[`name`](Workload.md#name) |
| <a id="summary"></a> `summary?` | `readonly` | `string` | One line about the workload, for the reference and the OpenAPI `summary`. | [`Workload`](Workload.md).[`summary`](Workload.md#summary) |
| <a id="description"></a> `description?` | `readonly` | `string` | A paragraph about the workload, for the reference and the OpenAPI `description`. | [`Workload`](Workload.md).[`description`](Workload.md#description) |
| <a id="trigger"></a> `trigger` | `readonly` | `Record`\<`string`, `unknown`\> | Kind-specific facts (method and path, schedule, topic, …), as the manifest carries them. | [`Workload`](Workload.md).[`trigger`](Workload.md#trigger) |
| <a id="contracts"></a> `contracts` | `readonly` | \{ `params?`: [`AnySchema`](../type-aliases/AnySchema.md); `query?`: [`AnySchema`](../type-aliases/AnySchema.md); `headers?`: [`AnySchema`](../type-aliases/AnySchema.md); `body?`: [`AnySchema`](../type-aliases/AnySchema.md); `input?`: [`AnySchema`](../type-aliases/AnySchema.md); `message?`: [`AnySchema`](../type-aliases/AnySchema.md); `response?`: `Record`\<`number`, [`AnySchema`](../type-aliases/AnySchema.md)\>; `events?`: `Record`\<`string`, [`AnySchema`](../type-aliases/AnySchema.md)\>; \} | - | [`Workload`](Workload.md).[`contracts`](Workload.md#contracts) |
| `contracts.params?` | `public` | [`AnySchema`](../type-aliases/AnySchema.md) | - | - |
| `contracts.query?` | `public` | [`AnySchema`](../type-aliases/AnySchema.md) | - | - |
| `contracts.headers?` | `public` | [`AnySchema`](../type-aliases/AnySchema.md) | - | - |
| `contracts.body?` | `public` | [`AnySchema`](../type-aliases/AnySchema.md) | - | - |
| `contracts.input?` | `public` | [`AnySchema`](../type-aliases/AnySchema.md) | - | - |
| `contracts.message?` | `public` | [`AnySchema`](../type-aliases/AnySchema.md) | - | - |
| `contracts.response?` | `public` | `Record`\<`number`, [`AnySchema`](../type-aliases/AnySchema.md)\> | - | - |
| `contracts.events?` | `public` | `Record`\<`string`, [`AnySchema`](../type-aliases/AnySchema.md)\> | A stream's events by name (`http.stream({ events })`). | - |
| <a id="errors"></a> `errors` | `readonly` | [`DeclaredError`](DeclaredError.md)[] | - | [`Workload`](Workload.md).[`errors`](Workload.md#errors) |
| <a id="responseheaders"></a> `responseHeaders?` | `readonly` | [`ResponseHeaderDocs`](../type-aliases/ResponseHeaderDocs.md) | Documented response headers per status (HTTP, raw and stream workloads). | [`Workload`](Workload.md).[`responseHeaders`](Workload.md#responseheaders) |
| <a id="operationid"></a> `operationId?` | `readonly` | `string` | The chosen OpenAPI `operationId`, when the declaration set one. | [`Workload`](Workload.md).[`operationId`](Workload.md#operationid) |
| <a id="auth"></a> `auth?` | `readonly` | [`AuthDeclaration`](AuthDeclaration.md)\<`unknown`, readonly [`ResourceDeclaration`](ResourceDeclaration.md)\<`string`, `unknown`\>[]\> | - | [`Workload`](Workload.md).[`auth`](Workload.md#auth) |
| <a id="resources"></a> `resources` | `readonly` | [`ResourceDeclaration`](ResourceDeclaration.md)\<`string`, `unknown`\>[] | - | [`Workload`](Workload.md).[`resources`](Workload.md#resources) |
| <a id="dispatches"></a> `dispatches` | `readonly` | [`Workload`](Workload.md)[] | - | [`Workload`](Workload.md).[`dispatches`](Workload.md#dispatches) |
| <a id="publishes"></a> `publishes` | `readonly` | `string`[] | Queue topics this workload publishes to (`publishes(...)`). | [`Workload`](Workload.md).[`publishes`](Workload.md#publishes) |
| <a id="policies"></a> `policies` | `readonly` | [`WorkloadPolicies`](WorkloadPolicies.md) | - | [`Workload`](Workload.md).[`policies`](Workload.md#policies) |
