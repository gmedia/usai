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
use tracing_subscriber::fmt::{FormatEvent, FormatFields};
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

        // Written key by key, in a deliberate order, because `serde_json`'s
        // map is sorted and a line that begins `{"application"` is not the
        // one shippers were configured against: 0.0.9 moved `timestamp` out
        // of first position, which breaks any pipeline that anchors on a
        // line prefix. Timestamp, level, message, target, then everything
        // the event carried — the shape 0.0.8 had.
        let mut out = String::with_capacity(256);
        out.push('{');
        out.push_str("\"timestamp\":");
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| {
                // RFC 3339 in UTC, to the microsecond (0.0.8's precision:
                // two lines in the same millisecond have to stay orderable),
                // without pulling a date library into the CLI.
                let secs = d.as_secs() as i64;
                let us = d.subsec_micros();
                let days = secs.div_euclid(86_400);
                let sod = secs.rem_euclid(86_400);
                let (y, m, dd) = civil_from_days(days);
                format!(
                    "{y:04}-{m:02}-{dd:02}T{:02}:{:02}:{:02}.{us:06}Z",
                    sod / 3600,
                    (sod % 3600) / 60,
                    sod % 60
                )
            })
            .unwrap_or_default();
        out.push_str(&Value::String(stamp).to_string());
        let put = |key: &str, value: &Value, out: &mut String| {
            out.push(',');
            out.push_str(&Value::String(key.to_owned()).to_string());
            out.push(':');
            out.push_str(&value.to_string());
        };
        put(
            "level",
            &Value::String(meta.level().as_str().to_owned()),
            &mut out,
        );
        // `message` is the event's own text; it is in `fields` under that
        // name, and it belongs next to the level rather than wherever the
        // alphabet puts it.
        let mut rest = fields.map;
        if let Some(message) = rest.remove("message") {
            put("message", &message, &mut out);
        }
        put("target", &Value::String(meta.target().to_owned()), &mut out);
        // Spans: the runtime creates none of its own (every fact a line needs
        // is on the event), and a dependency's span name is worth more than a
        // half-parsed rendering of its fields.
        if let Some(span) = ctx.lookup_current()
            && span.metadata().target() != meta.target()
        {
            put("span", &Value::String(span.name().to_owned()), &mut out);
        }
        for (k, v) in &rest {
            put(k, v, &mut out);
        }
        out.push('}');
        writeln!(writer, "{out}")
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
