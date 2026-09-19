[@sakaladev/usai](../../README.md) / [index](../README.md) / SeederDeclaration

# Interface: SeederDeclaration

A seeder file's default export. Discovered by `usai db seed`, run as
finite work with access to the declared resources; never part of
startup.

## Properties

| Property | Modifier | Type |
| ------ | ------ | ------ |
| <a id="__usai"></a> `__usai` | `readonly` | `"seeder"` |
| <a id="resources"></a> `resources` | `readonly` | readonly [`ResourceDeclaration`](ResourceDeclaration.md)[] |
| <a id="run"></a> `run` | `readonly` | (`ctx`: [`SeederContext`](SeederContext.md)) => `unknown` |
