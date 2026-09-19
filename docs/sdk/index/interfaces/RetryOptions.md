[@sakaladev/usai](../../README.md) / [index](../README.md) / RetryOptions

# Interface: RetryOptions

Retry policy of a queue consumer.

## Properties

| Property | Type | Description |
| ------ | ------ | ------ |
| <a id="maxattempts"></a> `maxAttempts` | `number` | Total attempts including the first. Default 1: failure is terminal. |
| <a id="backoff"></a> `backoff?` | `"fixed"` \| `"exponential"` | `fixed` (default): `baseMs` between attempts; `exponential`: doubling from `baseMs`. |
| <a id="basems"></a> `baseMs?` | `number` | Base delay in ms (default 1000). |
