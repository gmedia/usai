[@sakaladev/usai](../../README.md) / [index](../README.md) / auth

# Variable: auth

```ts
const auth: {
  bearer: AuthDeclaration<P, R>;
  header: AuthDeclaration<P, R>;
  cookie: AuthDeclaration<P, R>;
  custom: AuthDeclaration<P, R>;
};
```

Declare an authentication boundary. Attach it to a workload with
`auth: <declaration>`; `resolve` runs before the handler, with the
workload's `ctx` — its declared resources plus the scheme's own
`resources`, typed — and `ctx.request`, and the
principal it returns is `ctx.auth`, typed. A missing credential or a
thrown `errors.unauthorized()` answers 401 and the handler never runs.
Authentication (who) lives here; authorization (may they) is business
logic in the handler. One declaration is reused by reference across
endpoints; its `name` is the OpenAPI security scheme. In v0 the resolver
is application code and runs inside the request's world (ADR-0004);
the rest of the boundary — routing, decoding, schema validation — runs
before any world exists.

## Type Declaration

| Name | Type | Description |
| ------ | ------ | ------ |
| `bearer()` | (`options`: [`BearerOptions`](../interfaces/BearerOptions.md)\<`P`, `R`\>) => [`AuthDeclaration`](../interfaces/AuthDeclaration.md)\<`P`, `R`\> | `Authorization: Bearer <token>`; `resolve` receives the token. |
| `header()` | (`options`: [`HeaderOptions`](../interfaces/HeaderOptions.md)\<`P`, `R`\>) => [`AuthDeclaration`](../interfaces/AuthDeclaration.md)\<`P`, `R`\> | A credential in the named header (an API key); `resolve` receives its value. |
| `cookie()` | (`options`: [`CookieOptions`](../interfaces/CookieOptions.md)\<`P`, `R`\>) => [`AuthDeclaration`](../interfaces/AuthDeclaration.md)\<`P`, `R`\> | A cookie (`cookie: "sid"`); `resolve` receives its value. A missing cookie is a 401 before the handler runs. Login sets it with `http.response(200, body, { "set-cookie": cookies.serialize("sid", value, { maxAge }) })`, logout clears it with `maxAge: 0`. The OpenAPI document says `apiKey in: cookie` and the reference's request panel sends the browser's cookie with `credentials: include`. |
| `custom()` | (`options`: [`CustomOptions`](../interfaces/CustomOptions.md)\<`P`, `R`\>) => [`AuthDeclaration`](../interfaces/AuthDeclaration.md)\<`P`, `R`\> | Anything else: `resolve` reads the request itself. |

## Example

```ts
export const session = auth.bearer({
  name: "session",
  description: "The token from POST /login",
  resources: [db],                       // the resolver's own; every route using the scheme gets them
  resolve: async (ctx, token) => {
    const row = await ctx.resources.db.one<Principal>("select … from sessions where token = $1", [token]);
    if (!row) throw errors.unauthorized("unknown or expired token");
    return row;
  },
});
export const me = http.get("/me", { auth: session }, async (ctx) => ctx.auth);
```
