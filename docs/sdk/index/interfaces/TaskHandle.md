[@sakaladev/usai](../../README.md) / [index](../README.md) / TaskHandle

# Interface: TaskHandle

`ctx.tasks`: the two ways to start a task, and the whole difference
between them is who owns the child world.

## Methods

### invoke()

```ts
invoke<T = unknown>(task: Workload, input?: unknown): Promise<T>;
```

*Owned** invocation: the task runs in a fresh world, this world waits
for its result, and cancelling this world cancels the child. The
child's thrown [UsaiError](../classes/UsaiError.md) is rethrown here. Use it when the
response depends on the task.

#### Type Parameters

| Type Parameter | Default type |
| ------ | ------ |
| `T` | `unknown` |

#### Parameters

| Parameter | Type |
| ------ | ------ |
| `task` | [`Workload`](Workload.md) |
| `input?` | `unknown` |

#### Returns

`Promise`\<`T`\>

***

### dispatch()

```ts
dispatch(task: Workload, input?: unknown): Promise<{
  id: string;
}>;
```

*Ownership transfer**: the task runtime owns the child, which starts
once this world commits; this world may end. Resolves with the child's
id as soon as the hand-off is accepted — not durable across a runtime
restart (publish to a queue for that). Declare the edge with
`dispatches(from, task)`.

#### Parameters

| Parameter | Type |
| ------ | ------ |
| `task` | [`Workload`](Workload.md) |
| `input?` | `unknown` |

#### Returns

`Promise`\<\{
  `id`: `string`;
\}\>
