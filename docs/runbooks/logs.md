# The log line: what is in it, what to label, what to leave alone

Written because an observability engineer reconstructed all of this by
parsing 92 lines by hand (round 27, `docs/STATUS.md`) — the field set, the
cardinality, and the fact that one concept has three key names depending on
who wrote the line.

Run with `--log-format json` and everything below is one JSON object per
line, on **stderr**. Nothing is written to stdout, so a shipper reading the
journal or the container's stream gets a clean parse per line (`usai run`
emits one structured `serving` line instead of the human banner).

## The fields

| Field | Always? | What it is |
|---|---|---|
| `timestamp` | yes | RFC 3339, microsecond precision, first key in the object |
| `level` | yes | `ERROR`, `WARN`, `INFO`, `DEBUG` |
| `message` | yes | The event. One line whatever the content: a quote, a newline or a backslash is escaped, never wrapped |
| `target` | yes | `app` for the application's own `ctx.log`/`console.*`; `usai_runtime::…` for the runtime's. **This is the redaction boundary** |
| `workload` | most | `http:GET /items/:id`, `task:resize`, `queue:notify`. The route *template*, never the path a client sent |
| `world` | most | `world#42` — the unit of work. Unique **per process**, not per fleet |
| `request_id` | most | The client's `x-request-id` or the one the runtime minted. **The join key**, and it follows a hand-off: an invoked task, a dispatched task, a queue message and an outbound call all carry the same one |
| `fields` | app lines | A real nested object — the trailing plain object of `ctx.log.info("paid", { invoiceId })`. No second parse stage |
| `code`, `error`, `stack` | failures | The error's code, message and source-mapped stack |
| `queue`, `id`, `attempt` | queue lines | The topic, the message id, which attempt |
| `duration_ms`, `cpu_us`, `termination` | the trace line | See *During an incident* below |

## Labels, and what must stay a field

Labels are indexed and their cardinality is your storage bill. These are
bounded by the application's declaration — `usai inspect` tells you the
count before you deploy:

```yaml
# promtail / Grafana Alloy
- json:
    expressions: { level: level, target: target, workload: workload, ts: timestamp }
- labels: { level: , target: , workload: }
- timestamp: { source: ts, format: RFC3339Nano }
```

Everything else stays a **field**, extracted at query time. In particular:

- **`request_id` is never a label.** It is unbounded by construction — one
  value per request — and it is the field you query *by*, not group by.
- **`world` is not a label either**, and it is only unique within one
  process. Your `pod`/`instance` label is what disambiguates it across
  replicas.
- **`revision` is not on ordinary lines.** It appears on the start-up lines
  and on the per-world trace line, and nowhere else — you cannot split an
  ordinary log stream by revision during a rolling deploy.

**One concept, three spellings.** The same queue topic is `workload="queue:notify"`
on an application line, `queue="notify"` on a runtime queue line, and
`topic="notify"` on the metric. A pipeline that normalises them needs all
three.

**There is no field-size cap.** A 20 000-character value produces a
20 000-character line, whole. One `ctx.log.info("req", { body })` will
exhaust a per-stream ingest limit; truncate in the handler.

## During an incident

```bash
RUST_LOG='warn,usai_runtime::observability=debug' usai run …
```

The leading `warn,` is not optional: a `RUST_LOG` value **replaces** the
default filter rather than adding to it, so without it you lose every
`application error`, `task failed` and `message dead-lettered` line — at the
moment you turned the flag on to read them. (The one exception is the
application's own lines: a filter that names no level for `app` keeps them at
INFO, so `RUST_LOG=warn` never deletes `ctx.log`.)

What that adds is one `world trace` line per unit of work:

```json
{"message":"world trace","world":"world#75","workload":"http:POST /orders",
 "request_id":"…","termination":"completed","duration_ms":41.2,"cpu_us":2210,
 "revision":"rev2","children":["task:validate[Owned]","task:audit[Transferred]"]}
```

`cpu_us` close to `duration_ms` means the handler was **computing**; far
below it means it was **waiting** (`slow-route.md`). `children` is the
ownership tree for that request — what it invoked and what it handed off.
Plain `RUST_LOG=usai_runtime=debug` adds six more lines per request and is
not something to leave on.

## The queries worth saving

```logql
# Everything that happened for one request, across every hop
{app="my-app"} | json | request_id="RID-123"

# Application failures only (the runtime's own lines are a different target)
{app="my-app", level="ERROR", target!="app"} | json

# One route's own log, including the tasks it handed off
{app="my-app"} | json | workload=~"http:POST /orders|task:.*" | request_id="RID-123"
```

The pivot an on-call makes is: dashboard shows 5xx → find the
`application error` line → take its `request_id` → run the first query.
Every line that failure touched carries that id.
