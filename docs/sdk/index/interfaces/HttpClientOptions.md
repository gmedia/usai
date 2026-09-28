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
| <a id="tls"></a> `tls?` | \{ `caFile?`: `string`; `caFileEnv?`: `string`; `clientCertFile?`: `string`; `clientCertFileEnv?`: `string`; `clientKeyFile?`: `string`; `clientKeyFileEnv?`: `string`; \} | Trust and identity for TLS, the same shape `postgres` takes. A private certificate authority and a client certificate are ordinary on a management network — a router's REST API, a mutually-authenticated internal service — and `postgres` has had `tls.caFile` since it shipped, so an `httpClient` that could not do it was an asymmetry rather than a decision. There is deliberately no way to skip verification: every option here **adds** trust. `caFile` is added to the built-in roots rather than replacing them, and `clientCertFile`/`clientKeyFile` go together. All of them are read at **activation**, so a wrong path or a file that is not PEM stops the deployment instead of surfacing as a failed request later. Each one has an `…Env` form, the way `baseUrl` has `baseUrlEnv`: the path comes from the environment instead of the declaration. Deployments differ — a CA per site, a certificate mounted somewhere else in staging — while the code does not, and a literal path forces every environment to mount the file where the source happened to say. **An `…Env` variable that is unset means "no extra trust", not a failure**, so a development box that never calls the client still activates; a variable that *is* set and names a missing or unreadable file still stops activation, because that is a deployment saying one thing and meaning another. Setting both the literal and the `…Env` form is a build error. `const routers = httpClient("routers", { baseUrlEnv: "ROUTEROS_URL", tls: { caFileEnv: "ROUTER_CA_FILE" }, });` |
| `tls.caFile?` | `string` | - |
| `tls.caFileEnv?` | `string` | - |
| `tls.clientCertFile?` | `string` | - |
| `tls.clientCertFileEnv?` | `string` | - |
| `tls.clientKeyFile?` | `string` | - |
| `tls.clientKeyFileEnv?` | `string` | - |
| <a id="optional"></a> `optional?` | `boolean` | This deployment may not have the thing on the other end. A part can be absent — a lab without a collector, a site whose bridge is not up yet — and the application should then say "not configured", not refuse to start. Every variable a resource names is normally required at activation, which is right for a database and wrong for an integration that is genuinely optional. With `optional: true` and **none** of this client's variables set, the resource is not opened and `ctx.resources.<name>` is `undefined`. The type says so, so a handler has to decide what to do rather than find out at runtime. Set *some* of them and activation still fails: that is a half-configured integration, not an absent one. `const collector = httpClient("collector", { baseUrlEnv: "COLLECTOR_URL", optional: true }); export const system = http.get("/system", { resources: [collector] }, async (ctx) => { if (!ctx.resources.collector) return { collector: "not configured" }; const res = await ctx.resources.collector.fetch("/metrics"); return { collector: res.ok ? "ok" : "unreachable" }; });` |
| <a id="allowprivatenetwork"></a> `allowPrivateNetwork?` | `boolean` | Let a client **without** a `baseUrl` reach loopback, private, link-local and unique-local addresses. Off by default. The only reason a client without a `baseUrl` exists is that the destination comes from the application's own data — a tenant-configured webhook — which makes it attacker-influenced by construction, and `http://169.254.169.254/…`, `http://127.0.0.1:3900` and an internal service's name are all requests your application would make on the caller's behalf. Refused with `destination_refused`. Set it only when the client really does call internal addresses chosen at runtime. A client that names its destination (`baseUrl` / `baseUrlEnv`) is pinned to one origin already and is never checked. |
