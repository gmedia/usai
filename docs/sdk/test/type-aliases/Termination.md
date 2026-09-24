[@sakaladev/usai](../../README.md) / [test](../README.md) / Termination

# Type Alias: Termination

```ts
type Termination = 
  | "completed"
  | "deadline-exceeded"
  | string & {
};
```

How a world ended. The four the runtime names, plus the open form so a
future one is a value and not a type error.
