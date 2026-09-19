[@sakaladev/usai](../../README.md) / [index](../README.md) / password

# Variable: password

```ts
const password: {
  hash: Promise<string>;
  verify: Promise<boolean>;
};
```

Password hashing as a host operation. Argon2id is meant to be expensive,
so it runs on the runtime's blocking pool, never on a world's thread;
each call is one owned operation, cancelled with the world. The hash is
a PHC string (`$argon2id$v=19$m=19456,t=2,p=1$…`) to store as text.

## Type Declaration

| Name | Type | Description |
| ------ | ------ | ------ |
| `hash()` | (`plain`: `string`) => `Promise`\<`string`\> | Argon2id, random 16-byte salt, the runtime's default cost. |
| `verify()` | (`plain`: `string`, `hash`: `string`) => `Promise`\<`boolean`\> | Constant-time verification against a PHC hash; `false`, never a throw, for a wrong password. |

## Example

```ts
const hash = await password.hash(ctx.body.password);
// later
if (!(await password.verify(ctx.body.password, row.password_hash))) throw errors.unauthorized();
```
