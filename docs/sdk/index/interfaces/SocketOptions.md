[@sakaladev/usai](../../README.md) / [index](../README.md) / SocketOptions

# Interface: SocketOptions\<I *extends* [`AnySchema`](../type-aliases/AnySchema.md) \| `undefined`, O *extends* [`AnySchema`](../type-aliases/AnySchema.md) \| `undefined`\>

Options for [socket](../functions/socket.md).

## Extends

- [`WorkloadPolicies`](WorkloadPolicies.md)

## Type Parameters

| Type Parameter |
| ------ |
| `I` *extends* [`AnySchema`](../type-aliases/AnySchema.md) \| `undefined` |
| `O` *extends* [`AnySchema`](../type-aliases/AnySchema.md) \| `undefined` |

## Properties

| Property | Type | Description | Inherited from |
| ------ | ------ | ------ | ------ |
| <a id="incoming"></a> `incoming?` | `I` | Schema for messages from the client. An invalid message is answered with a `validation_failed` error envelope and dropped; the connection stays open. | - |
| <a id="outgoing"></a> `outgoing?` | `O` | Schema for messages to the client. | - |
| <a id="auth"></a> `auth?` | [`AuthDeclaration`](AuthDeclaration.md)\<`unknown`\> | - | - |
| <a id="resources"></a> `resources?` | [`ResourceDeclaration`](ResourceDeclaration.md)[] | - | - |
| <a id="timeout"></a> `timeout?` | `string` \| `number` | Per-invocation deadline (`"5s"`, `"500ms"`, or milliseconds). The world is cancelled when it passes; HTTP callers get 504. Undeclared: the runtime default (30 s) for requests, none for the other kinds. | [`WorkloadPolicies`](WorkloadPolicies.md).[`timeout`](WorkloadPolicies.md#timeout) |
| <a id="concurrency"></a> `concurrency?` | `number` | How many worlds of this workload may run at once; the next request is refused with 503 `capacity_exhausted`, not queued (ADR-0012). | [`WorkloadPolicies`](WorkloadPolicies.md).[`concurrency`](WorkloadPolicies.md#concurrency) |
