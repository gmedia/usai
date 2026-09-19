[@sakaladev/usai](../../README.md) / [index](../README.md) / HttpClientOptions

# Interface: HttpClientOptions

Options for [httpClient](../functions/httpClient.md). Secrets never go in the declaration:
name the environment variables that hold them.

## Properties

| Property | Type | Description |
| ------ | ------ | ------ |
| <a id="baseurl"></a> `baseUrl?` | `string` | Every request is relative to it; another origin is refused. Without it the client may call any http(s) URL. |
| <a id="baseurlenv"></a> `baseUrlEnv?` | `string` | Environment variable holding the base URL (staging and production differ; the declaration does not). |
| <a id="timeoutms"></a> `timeoutMs?` | `number` | Per-request timeout in milliseconds (default 10 000). |
| <a id="maxconcurrent"></a> `maxConcurrent?` | `number` | In-flight bound; the next request is refused with 503, not queued. |
| <a id="headers"></a> `headers?` | `Record`\<`string`, `string`\> | Static headers on every request. |
| <a id="bearertokenenv"></a> `bearerTokenEnv?` | `string` | Environment variable whose value is sent as `Authorization: Bearer …`. |
