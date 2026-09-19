[@sakaladev/usai](../../README.md) / [index](../README.md) / errors

# Variable: errors

```ts
const errors: {
  badRequest: (message?: string, details?: unknown) => UsaiError;
  unauthorized: (message?: string, details?: unknown) => UsaiError;
  forbidden: (message?: string, details?: unknown) => UsaiError;
  notFound: (message?: string, details?: unknown) => UsaiError;
  conflict: (message?: string, details?: unknown) => UsaiError;
  unprocessable: (message?: string, details?: unknown) => UsaiError;
  tooManyRequests: (message?: string, details?: unknown) => UsaiError;
  internal: (message?: string, details?: unknown) => UsaiError;
  unavailable: (message?: string, details?: unknown) => UsaiError;
  custom: UsaiError;
};
```

Constructors for the common [UsaiError](../classes/UsaiError.md)s. Each takes an optional
message (default: the code, spaced) and `details` (any JSON, echoed to
the client — keep it safe to show). List the codes a workload throws in
its `errors` option so the reference and the OpenAPI document say so.

## Type Declaration

| Name | Type | Description |
| ------ | ------ | ------ |
| <a id="property-badrequest"></a> `badRequest()` | (`message?`: `string`, `details?`: `unknown`) => [`UsaiError`](../classes/UsaiError.md) | 400 `bad_request`. |
| <a id="property-unauthorized"></a> `unauthorized()` | (`message?`: `string`, `details?`: `unknown`) => [`UsaiError`](../classes/UsaiError.md) | 401 `unauthorized` (what an auth resolver throws). |
| <a id="property-forbidden"></a> `forbidden()` | (`message?`: `string`, `details?`: `unknown`) => [`UsaiError`](../classes/UsaiError.md) | 403 `forbidden`. |
| <a id="property-notfound"></a> `notFound()` | (`message?`: `string`, `details?`: `unknown`) => [`UsaiError`](../classes/UsaiError.md) | 404 `not_found`. |
| <a id="property-conflict"></a> `conflict()` | (`message?`: `string`, `details?`: `unknown`) => [`UsaiError`](../classes/UsaiError.md) | 409 `conflict`. |
| <a id="property-unprocessable"></a> `unprocessable()` | (`message?`: `string`, `details?`: `unknown`) => [`UsaiError`](../classes/UsaiError.md) | 422 `unprocessable`. |
| <a id="property-toomanyrequests"></a> `tooManyRequests()` | (`message?`: `string`, `details?`: `unknown`) => [`UsaiError`](../classes/UsaiError.md) | 429 `too_many_requests`. |
| <a id="property-internal"></a> `internal()` | (`message?`: `string`, `details?`: `unknown`) => [`UsaiError`](../classes/UsaiError.md) | 500 `internal`. |
| <a id="property-unavailable"></a> `unavailable()` | (`message?`: `string`, `details?`: `unknown`) => [`UsaiError`](../classes/UsaiError.md) | 503 `unavailable` (a dependency is down; retryable). |
| `custom()` | ( `code`: `string`, `status`: `number`, `message?`: `string`, `details?`: `unknown` ) => [`UsaiError`](../classes/UsaiError.md) | A custom declared error. `code` should also be listed in the workload's `errors`. |

## Example

```ts
const invoice = await ctx.resources.db.one("select … where id = $1", [ctx.params.id]); // resources: [db]
if (!invoice) throw errors.notFound("invoice not found", { id: ctx.params.id });
if (invoice.status !== "draft") throw errors.conflict("only a draft can be issued");
```
