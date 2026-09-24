[@sakaladev/usai](../../README.md) / [index](../README.md) / TaskHandle

# Interface: TaskHandle

`ctx.tasks`: the two ways to start a task, and the whole difference
between them is who owns the child world.

## Methods

### invoke()

```ts
invoke<W extends Workload>(task: W, ...input: TaskInputArgs<W>): Promise<TaskOutput<W>>;
```

An **owned** invocation: the task runs in a fresh world, this world waits
for its result, and cancelling this world cancels the child. The
child's thrown [UsaiError](../classes/UsaiError.md) is rethrown here. Use it when the
response depends on the task.

#### Type Parameters

| Type Parameter |
| ------ |
| `W` *extends* [`Workload`](Workload.md) |

#### Parameters

| Parameter | Type |
| ------ | ------ |
| `task` | `W` |
| ...`input` | `TaskInputArgs`\<`W`\> |

#### Returns

`Promise`\<[`TaskOutput`](../type-aliases/TaskOutput.md)\<`W`\>\>

***

### dispatch()

```ts
dispatch<W extends Workload>(task: W, ...input: TaskInputArgs<W>): Promise<{
  id: string;
}>;
```

An **ownership transfer**: the task runtime owns the child, which
starts once this world commits (its handler returned; for HTTP, the
response is committed) — a world that throws hands nothing off. This
world may end. Resolves with the child's id as soon as the hand-off is
accepted; rejects with `capacity_exhausted` when the task's
`concurrency` is full and `unknown_task` for a name that does not
exist. Nobody receives the child's return value. Not durable across a
runtime restart (publish to a queue for that). Declare the edge with
`dispatches(from, task)`.

#### Type Parameters

| Type Parameter |
| ------ |
| `W` *extends* [`Workload`](Workload.md) |

#### Parameters

| Parameter | Type |
| ------ | ------ |
| `task` | `W` |
| ...`input` | `TaskInputArgs`\<`W`\> |

#### Returns

`Promise`\<\{
  `id`: `string`;
\}\>
