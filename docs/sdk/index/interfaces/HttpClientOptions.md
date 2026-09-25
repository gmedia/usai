[@sakaladev/usai](../../README.md) / [index](../README.md) / HttpClientOptions

# Interface: HttpClientOptions

Options for [httpClient](../functions/httpClient.md). Secrets never go in the declaration:
name the environment variables that hold them.

## Properties

| Property | Type | Description |
| ------ | ------ | ------ |
| <a id="baseurl"></a> `baseUrl?` | `string` | Every request is relative to it; another origin is refused. Without it the client may call any **public** http(s) URL — this host's own network is refused (see [HttpClientOptions.allowPrivateNetwork](#allowprivatenetwork)). |
| <a id="baseurlenv"></a> `baseUrlEnv?` | `string` | Environment variable holding the base URL (staging and production differ; the declaration does not). |
| <a id="timeoutms"></a> `timeoutMs?` | `number` | Per-request timeout in milliseconds (default 10 000). |
| <a id="maxconcurrent"></a> `maxConcurrent?` | `number` | In-flight bound; the next request is refused with 503, not queued. |
| <a id="headers"></a> `headers?` | `Record`\<`string`, `string`\> | Static headers on every request. |
| <a id="bearertokenenv"></a> `bearerTokenEnv?` | `string` | Environment variable whose value is sent as `Authorization: Bearer …`. |
| <a id="tls"></a> `tls?` | \{ `caFile?`: `string`; `clientCertFile?`: `string`; `clientKeyFile?`: `string`; \} | Trust and identity for TLS, the same shape `postgres` takes. A private certificate authority and a client certificate are ordinary on a management network — a router's REST API, a mutually-authenticated internal service — and `postgres` has had `tls.caFile` since it shipped, so an `httpClient` that could not do it was an asymmetry rather than a decision. There is deliberately no way to skip verification: every option here **adds** trust. `caFile` is added to the built-in roots rather than replacing them, and `clientCertFile`/`clientKeyFile` go together. All of them are read at **activation**, so a wrong path or a file that is not PEM stops the deployment instead of surfacing as a failed request later. `const routers = httpClient("routers", { baseUrlEnv: "ROUTEROS_URL", tls: { caFile: "/etc/usai/router-ca.pem" }, });` |
| `tls.caFile?` | `string` | - |
| `tls.clientCertFile?` | `string` | - |
| `tls.clientKeyFile?` | `string` | - |
| <a id="allowprivatenetwork"></a> `allowPrivateNetwork?` | `boolean` | Let a client **without** a `baseUrl` reach loopback, private, link-local and unique-local addresses. Off by default. The only reason a client without a `baseUrl` exists is that the destination comes from the application's own data — a tenant-configured webhook — which makes it attacker-influenced by construction, and `http://169.254.169.254/…`, `http://127.0.0.1:3900` and an internal service's name are all requests your application would make on the caller's behalf. Refused with `destination_refused`. Set it only when the client really does call internal addresses chosen at runtime. A client that names its destination (`baseUrl` / `baseUrlEnv`) is pinned to one origin already and is never checked. |
