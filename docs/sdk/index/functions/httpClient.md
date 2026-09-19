[@sakaladev/usai](../../README.md) / [index](../README.md) / httpClient

# Function: httpClient()

```ts
function httpClient<N extends string>(name: N, options?: HttpClientOptions): HttpClientDeclaration<N>;
```

Declare an outbound HTTP client. There is no global `fetch` in a world;
this is how an application calls another service, and the destination
is part of the application's declared shape. Each `fetch` is one leased
operation: cancelled with the world, bounded by the smaller of the
world's deadline and `timeoutMs`, counted against `maxConcurrent` (the
next request is refused, not queued). With `baseUrl`/`baseUrlEnv` the
origin is pinned and any other origin is `origin_refused` before the
request leaves. A non-2xx status is data (`ok: false`), not an exception;
connection failures and timeouts throw and map to 503 for HTTP callers.

## Type Parameters

| Type Parameter |
| ------ |
| `N` *extends* `string` |

## Parameters

| Parameter | Type |
| ------ | ------ |
| `name` | `N` |
| `options` | [`HttpClientOptions`](../interfaces/HttpClientOptions.md) |

## Returns

[`HttpClientDeclaration`](../interfaces/HttpClientDeclaration.md)\<`N`\>

## Example

```ts
export const mailer = httpClient("mailer", { baseUrlEnv: "MAILER_URL", bearerTokenEnv: "MAILER_TOKEN", timeoutMs: 5_000, maxConcurrent: 8 });
export const send = task("send-mail", { input: Mail, resources: [mailer] }, async (ctx) => {
  const res = await ctx.resources.mailer.fetch("/v1/send", { method: "POST", json: ctx.input });
  if (!res.ok) throw errors.unavailable(`mailer answered ${res.status}`);
});
```
