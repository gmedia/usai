[@sakaladev/usai](../../README.md) / [index](../README.md) / GUEST\_ABI

# Variable: GUEST\_ABI

```ts
const GUEST_ABI: 1;
```

The host↔guest contract this SDK's in-world runtime speaks
(`docs/GUEST-ABI.md`). Stamped into `builtWith.abi`; a runtime with a
different bridge refuses the artifact at install instead of faulting
every world.
