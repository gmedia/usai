[@sakaladev/usai](../../README.md) / [index](../README.md) / CronOptions

# Interface: CronOptions

Options for [cron](../functions/cron.md).

## Extends

- [`WorkloadPolicies`](WorkloadPolicies.md)

## Properties

| Property | Type | Description | Inherited from |
| ------ | ------ | ------ | ------ |
| <a id="timeout"></a> `timeout?` | `string` \| `number` | Per-invocation deadline (`"5s"`, `"500ms"`, or milliseconds). The world is cancelled when it passes; HTTP callers get 504. Undeclared: the runtime default (30 s) for requests, none for the other kinds. | [`WorkloadPolicies`](WorkloadPolicies.md).[`timeout`](WorkloadPolicies.md#timeout) |
| <a id="concurrency"></a> `concurrency?` | `number` | How many worlds of this workload may run at once; the next request is refused with 503 `capacity_exhausted`, not queued (ADR-0012). | [`WorkloadPolicies`](WorkloadPolicies.md).[`concurrency`](WorkloadPolicies.md#concurrency) |
| <a id="schedule"></a> `schedule` | `string` | Cron expression, UTC: five fields (`minute hour day-of-month month day-of-week`) or six with leading seconds. Validated at install. | - |
| <a id="overlap"></a> `overlap?` | `"allow"` \| `"skip"` | What to do when a tick is due while the previous one still runs. `skip` (default) drops the tick; `allow` starts another world. | - |
| <a id="resources"></a> `resources?` | [`ResourceDeclaration`](ResourceDeclaration.md)[] | Resources the tick leases. | - |
