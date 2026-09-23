//! `--log-format json`: one JSON object per line, and the application's own
//! structured fields as an **object** rather than a string.
//!
//! `tracing_subscriber`'s JSON formatter renders every field value as it was
//! recorded, and the guest hands its fields over as JSON text
//! (`console.info("paid", { invoiceId })` → `fields = "{\"invoiceId\":…}"`),
//! so a shipper had to parse the line and then parse one of its values again.
//! This formatter parses it once, here, where the string is known to be JSON.

use serde_json::{Map, Value};
use std::fmt;
use tracing::field::{Field, Visit};
use tracing::{Event, Subscriber};
use tracing_subscriber::fmt::format::Writer;
use tracing_subscriber::fmt::{FormatEvent, FormatFields, FormattedFields};
use tracing_subscriber::registry::LookupSpan;

pub struct JsonLine;

#[derive(Default)]
struct Fields {
    map: Map<String, Value>,
}

impl Visit for Fields {
    fn record_str(&mut self, field: &Field, value: &str) {
        self.put(field.name(), Value::String(value.to_owned()));
    }
    fn record_bool(&mut self, field: &Field, value: bool) {
        self.put(field.name(), Value::Bool(value));
    }
    fn record_i64(&mut self, field: &Field, value: i64) {
        self.put(field.name(), Value::from(value));
    }
    fn record_u64(&mut self, field: &Field, value: u64) {
        self.put(field.name(), Value::from(value));
    }
    fn record_f64(&mut self, field: &Field, value: f64) {
        self.put(field.name(), Value::from(value));
    }
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        self.put(field.name(), Value::String(format!("{value:?}")));
    }
    fn record_error(&mut self, field: &Field, value: &(dyn std::error::Error + 'static)) {
        self.put(field.name(), Value::String(value.to_string()));
    }
}

impl Fields {
    fn put(&mut self, name: &str, value: Value) {
        // The one field that is JSON text by construction: parse it, so a
        // shipper does not have to. Anything unparseable stays the string it
        // was — a log line is not the place to lose information.
        if name == "fields" {
            match &value {
                Value::String(s) if s.is_empty() => return,
                Value::String(s) => {
                    if let Ok(parsed @ Value::Object(_)) = serde_json::from_str::<Value>(s) {
                        self.map.insert(name.to_owned(), parsed);
                        return;
                    }
                }
                _ => {}
            }
        }
        // An empty request id says nothing; leaving it out keeps the line
        // about what happened.
        if matches!(&value, Value::String(s) if s.is_empty()) && name == "request_id" {
            return;
        }
        self.map.insert(name.to_owned(), value);
    }
}

impl<S, N> FormatEvent<S, N> for JsonLine
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        ctx: &tracing_subscriber::fmt::FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &Event<'_>,
    ) -> fmt::Result {
        let meta = event.metadata();
        let mut fields = Fields::default();
        event.record(&mut fields);

        let mut line = Map::new();
        line.insert(
            "timestamp".into(),
            Value::String(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| {
                        // RFC 3339 in UTC, to the millisecond, without pulling
                        // a date library into the CLI.
                        let secs = d.as_secs() as i64;
                        let ms = d.subsec_millis();
                        let days = secs.div_euclid(86_400);
                        let sod = secs.rem_euclid(86_400);
                        let (y, m, dd) = civil_from_days(days);
                        format!(
                            "{y:04}-{m:02}-{dd:02}T{:02}:{:02}:{:02}.{ms:03}Z",
                            sod / 3600,
                            (sod % 3600) / 60,
                            sod % 60
                        )
                    })
                    .unwrap_or_default(),
            ),
        );
        line.insert(
            "level".into(),
            Value::String(meta.level().as_str().to_owned()),
        );
        line.insert("target".into(), Value::String(meta.target().to_owned()));
        // The span the event happened in, when there is one: the runtime uses
        // spans for the world's trace.
        if let Some(span) = ctx.lookup_current()
            && let Some(formatted) = span.extensions().get::<FormattedFields<N>>()
            && !formatted.fields.is_empty()
            && let Ok(Value::Object(map)) = serde_json::from_str::<Value>(&format!(
                "{{{}}}",
                formatted.fields.replace('=', ":").replace(' ', ",")
            ))
        {
            line.insert("span".into(), Value::Object(map));
        }
        for (k, v) in fields.map {
            line.insert(k, v);
        }
        writeln!(writer, "{}", Value::Object(line))
    }
}

/// Days since the Unix epoch → (year, month, day). Howard Hinnant's
/// `civil_from_days`, which is the shortest correct way to do this without a
/// dependency.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}
