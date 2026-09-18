//! Observability derived from runtime truth (`GOAL.md` §44, contract C12).
//!
//! Everything here reads the same gauges, budgets, and revision state the
//! runtime uses to make decisions. Nothing is estimated. The detailed
//! per-world trace is emitted through `tracing` at `debug`, whose macros
//! evaluate their fields only when the level is enabled, so the disabled
//! path allocates nothing.

use std::fmt::Write as _;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::Serialize;

use crate::runtime::RuntimeStatus;
use crate::world::{Termination, WorkResult};

/// Latency histogram buckets (seconds), Prometheus-style cumulative.
pub const LATENCY_BUCKETS: [f64; 14] = [
    0.0005, 0.001, 0.0025, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0,
];

/// Why a request was refused before a world existed. Bounded set, so the
/// counters are fixed atomics rather than a map.
#[derive(Clone, Copy, Debug)]
pub enum Rejection {
    Route,
    Validation,
    Auth,
    Capacity,
    Draining,
    Other,
}

impl Rejection {
    pub fn from_code(code: &str) -> Self {
        match code {
            "route_not_found" | "method_not_allowed" => Rejection::Route,
            "validation_failed"
            | "invalid_body"
            | "invalid_json"
            | "unsupported_media_type"
            | "payload_too_large" => Rejection::Validation,
            "unauthorized" | "forbidden" => Rejection::Auth,
            "capacity_exhausted" => Rejection::Capacity,
            "revision_draining" => Rejection::Draining,
            _ => Rejection::Other,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Rejection::Route => "route",
            Rejection::Validation => "validation",
            Rejection::Auth => "auth",
            Rejection::Capacity => "capacity",
            Rejection::Draining => "draining",
            Rejection::Other => "other",
        }
    }
}

/// HTTP host counters. Class-level counters, a fixed-bucket latency
/// histogram and per-reason rejection counters: cheap (one atomic add
/// each), bounded, and enough for an error rate, a p99 and a "why are we
/// refusing" answer. Per-route detail belongs in traces.
#[derive(Debug, Default)]
pub struct HttpStats {
    pub requests: AtomicU64,
    pub responses_2xx: AtomicU64,
    pub responses_3xx: AtomicU64,
    pub responses_4xx: AtomicU64,
    pub responses_5xx: AtomicU64,
    pub rejected_before_world: AtomicU64,
    pub upgrades: AtomicU64,
    pub streams: AtomicU64,
    /// Counts per `LATENCY_BUCKETS` entry (non-cumulative), plus overflow.
    pub latency_buckets: [AtomicU64; 15],
    /// Sum of observed latencies, in microseconds.
    pub latency_sum_us: AtomicU64,
    pub rejections: [AtomicU64; 6],
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct HttpSnapshot {
    pub requests: u64,
    pub responses_2xx: u64,
    pub responses_3xx: u64,
    pub responses_4xx: u64,
    pub responses_5xx: u64,
    pub rejected_before_world: u64,
    pub upgrades: u64,
    pub streams: u64,
    /// Cumulative counts per `LATENCY_BUCKETS` entry, then `+Inf`.
    pub latency_cumulative: Vec<u64>,
    pub latency_sum_seconds: f64,
    /// Rejections by reason: route, validation, auth, capacity, draining, other.
    pub rejections: [u64; 6],
}

impl HttpStats {
    pub fn record(&self, status: u16, before_world: bool) {
        self.record_with(status, before_world, None, None);
    }

    pub fn record_with(
        &self,
        status: u16,
        before_world: bool,
        latency: Option<std::time::Duration>,
        rejection: Option<Rejection>,
    ) {
        self.requests.fetch_add(1, Ordering::Relaxed);
        let counter = match status {
            200..=299 => &self.responses_2xx,
            300..=399 => &self.responses_3xx,
            400..=499 => &self.responses_4xx,
            _ => &self.responses_5xx,
        };
        counter.fetch_add(1, Ordering::Relaxed);
        if let Some(latency) = latency {
            let seconds = latency.as_secs_f64();
            let index = LATENCY_BUCKETS.partition_point(|&b| b < seconds);
            self.latency_buckets[index].fetch_add(1, Ordering::Relaxed);
            self.latency_sum_us
                .fetch_add(latency.as_micros() as u64, Ordering::Relaxed);
        }
        if let Some(rejection) = rejection {
            self.rejections[rejection as usize].fetch_add(1, Ordering::Relaxed);
        }
        if before_world {
            self.rejected_before_world.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub fn snapshot(&self) -> HttpSnapshot {
        HttpSnapshot {
            requests: self.requests.load(Ordering::Relaxed),
            responses_2xx: self.responses_2xx.load(Ordering::Relaxed),
            responses_3xx: self.responses_3xx.load(Ordering::Relaxed),
            responses_4xx: self.responses_4xx.load(Ordering::Relaxed),
            responses_5xx: self.responses_5xx.load(Ordering::Relaxed),
            rejected_before_world: self.rejected_before_world.load(Ordering::Relaxed),
            upgrades: self.upgrades.load(Ordering::Relaxed),
            streams: self.streams.load(Ordering::Relaxed),
            latency_cumulative: {
                let mut total = 0;
                self.latency_buckets
                    .iter()
                    .map(|b| {
                        total += b.load(Ordering::Relaxed);
                        total
                    })
                    .collect()
            },
            latency_sum_seconds: self.latency_sum_us.load(Ordering::Relaxed) as f64 / 1e6,
            rejections: std::array::from_fn(|i| self.rejections[i].load(Ordering::Relaxed)),
        }
    }
}

/// The per-world trace record (`GOAL.md` §44). Emitted at `debug`; when
/// that level is off the macro short-circuits before any field is built.
pub fn trace_world(result: &WorkResult, revision: &str) {
    if !tracing::enabled!(tracing::Level::DEBUG) {
        return;
    }
    let termination = match &result.termination {
        Termination::Completed => "completed",
        Termination::Cancelled { .. } => "cancelled",
        Termination::DeadlineExceeded => "deadline-exceeded",
        Termination::Faulted { .. } => "faulted",
    };
    let outcome = match &result.outcome {
        Some(Ok(_)) => "ok",
        Some(Err(_)) => "error",
        None => "none",
    };
    let children: Vec<String> = result
        .children
        .iter()
        .map(|c| format!("{}[{:?}]", c.workload, c.relation))
        .collect();
    let violations: Vec<&str> = result.violations.iter().map(|v| v.code).collect();
    tracing::debug!(
        world = %result.world,
        workload = %result.workload,
        revision,
        termination,
        outcome,
        duration_ms = result.duration.as_millis() as u64,
        cpu_us = result.cpu.as_micros() as u64,
        completions_delivered = result.completions_delivered,
        completions_dropped = result.completions_dropped,
        children = ?children,
        violations = ?violations,
        "world trace"
    );
}

fn metric(out: &mut String, name: &str, help: &str, kind: &str, samples: &[(String, f64)]) {
    let _ = writeln!(out, "# HELP {name} {help}");
    let _ = writeln!(out, "# TYPE {name} {kind}");
    for (labels, value) in samples {
        if labels.is_empty() {
            let _ = writeln!(out, "{name} {value}");
        } else {
            let _ = writeln!(out, "{name}{{{labels}}} {value}");
        }
    }
}

fn label(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Prometheus text exposition of the runtime status. No timestamps, no
/// histograms in v0: gauges and counters the runtime already keeps.
pub fn render_prometheus(status: &RuntimeStatus, http: Option<&HttpSnapshot>) -> String {
    let mut out = String::new();
    let g = &status.gauges;
    metric(
        &mut out,
        "usai_worlds_live",
        "Execution worlds currently alive",
        "gauge",
        &[(String::new(), g.live_worlds as f64)],
    );
    metric(
        &mut out,
        "usai_worlds_created_total",
        "Execution worlds created",
        "counter",
        &[(String::new(), g.worlds_created as f64)],
    );
    metric(
        &mut out,
        "usai_guest_cpu_seconds_total",
        "Thread CPU time spent executing guest code, summed over worlds",
        "counter",
        &[(String::new(), g.guest_cpu_ns as f64 / 1e9)],
    );
    metric(
        &mut out,
        "usai_ops_live",
        "External operations with a live owner",
        "gauge",
        &[(String::new(), g.live_ops as f64)],
    );
    metric(
        &mut out,
        "usai_completions_total",
        "Operation completions by routing outcome",
        "counter",
        &[
            (
                "outcome=\"delivered\"".into(),
                g.completions_delivered as f64,
            ),
            (
                "outcome=\"dropped_late\"".into(),
                g.completions_dropped_late as f64,
            ),
            (
                "outcome=\"rejected_stale\"".into(),
                g.completions_rejected_stale as f64,
            ),
        ],
    );
    metric(
        &mut out,
        "usai_detached_work_total",
        "Finite worlds that ended with live asynchronous work",
        "counter",
        &[(String::new(), g.detached_work_detected as f64)],
    );
    metric(
        &mut out,
        "usai_world_budget",
        "Runtime world budget",
        "gauge",
        &[
            ("kind=\"in_use\"".into(), status.worlds_in_use as f64),
            ("kind=\"max\"".into(), status.worlds_max as f64),
        ],
    );
    let revisions: Vec<(String, f64)> = status
        .revisions
        .iter()
        .map(|r| {
            (
                format!(
                    "revision=\"{}\",application=\"{}\",state=\"{:?}\"",
                    r.id,
                    label(&r.application),
                    r.state
                )
                .to_lowercase(),
                r.in_flight as f64,
            )
        })
        .collect();
    if !revisions.is_empty() {
        metric(
            &mut out,
            "usai_revision_in_flight",
            "Work in flight per revision",
            "gauge",
            &revisions,
        );
    }
    let services: Vec<(String, f64)> = status
        .revisions
        .iter()
        .flat_map(|r| {
            r.services.iter().map(move |s| {
                (
                    format!(
                        "revision=\"{}\",service=\"{}\",state=\"{:?}\"",
                        r.id,
                        label(&s.name),
                        s.state
                    )
                    .to_lowercase(),
                    1.0,
                )
            })
        })
        .collect();
    if !services.is_empty() {
        metric(
            &mut out,
            "usai_service",
            "Service state (1 = in this state)",
            "gauge",
            &services,
        );
    }
    let queue: Vec<(String, f64)> = status
        .revisions
        .iter()
        .flat_map(|r| {
            [
                ("claimed", r.queue.claimed),
                ("done", r.queue.done),
                ("retried", r.queue.retried),
                ("dead", r.queue.dead),
                ("invalid", r.queue.invalid),
            ]
            .into_iter()
            .map(move |(state, n)| (format!("revision=\"{}\",state=\"{state}\"", r.id), n as f64))
        })
        .collect();
    if !queue.is_empty() {
        metric(
            &mut out,
            "usai_queue_messages_total",
            "Queue messages by outcome, per revision",
            "counter",
            &queue,
        );
    }
    let mut resources = Vec::new();
    for r in &status.resources {
        let base = format!(
            "kind=\"{}\",name=\"{}\"",
            label(&r.identity.kind),
            label(&r.identity.name)
        );
        resources.push((format!("{base},metric=\"in_use\""), r.in_use as f64));
        resources.push((format!("{base},metric=\"max\""), r.max as f64));
        resources.push((
            format!("{base},metric=\"quarantined\""),
            r.quarantined as f64,
        ));
    }
    if !resources.is_empty() {
        metric(
            &mut out,
            "usai_resource",
            "Resource manager state",
            "gauge",
            &resources,
        );
    }
    let t = &status.tasks;
    let tasks: Vec<(String, f64)> = ["queued", "running", "completed", "failed", "lost"]
        .iter()
        .filter_map(|k| {
            t.get(k)
                .and_then(|v| v.as_u64())
                .map(|v| (format!("state=\"{k}\""), v as f64))
        })
        .collect();
    metric(
        &mut out,
        "usai_tasks",
        "Dispatched task queue",
        "gauge",
        &tasks,
    );
    if let Some(h) = http {
        metric(
            &mut out,
            "usai_http_requests_total",
            "HTTP requests received",
            "counter",
            &[(String::new(), h.requests as f64)],
        );
        metric(
            &mut out,
            "usai_http_responses_total",
            "HTTP responses by status class",
            "counter",
            &[
                ("class=\"2xx\"".into(), h.responses_2xx as f64),
                ("class=\"3xx\"".into(), h.responses_3xx as f64),
                ("class=\"4xx\"".into(), h.responses_4xx as f64),
                ("class=\"5xx\"".into(), h.responses_5xx as f64),
            ],
        );
        metric(
            &mut out,
            "usai_http_rejected_before_world_total",
            "Requests refused before any world existed",
            "counter",
            &[(String::new(), h.rejected_before_world as f64)],
        );
        metric(
            &mut out,
            "usai_http_rejections_total",
            "Requests refused before a world existed, by reason",
            "counter",
            &[
                "route",
                "validation",
                "auth",
                "capacity",
                "draining",
                "other",
            ]
            .iter()
            .zip(h.rejections.iter())
            .map(|(reason, n)| (format!("reason=\"{reason}\""), *n as f64))
            .collect::<Vec<_>>(),
        );
        // Histogram of time from request receipt to response start, for
        // requests that reached the pipeline's end (rejections included).
        let mut samples: Vec<(String, f64)> = LATENCY_BUCKETS
            .iter()
            .zip(h.latency_cumulative.iter())
            .map(|(le, n)| (format!("le=\"{le}\""), *n as f64))
            .collect();
        let total = h.latency_cumulative.last().copied().unwrap_or(0);
        samples.push(("le=\"+Inf\"".into(), total as f64));
        let _ = writeln!(
            out,
            "# HELP usai_http_request_seconds Time to the response, by bucket\n# TYPE usai_http_request_seconds histogram"
        );
        for (labels, value) in &samples {
            let _ = writeln!(out, "usai_http_request_seconds_bucket{{{labels}}} {value}");
        }
        let _ = writeln!(
            out,
            "usai_http_request_seconds_sum {}",
            h.latency_sum_seconds
        );
        let _ = writeln!(out, "usai_http_request_seconds_count {total}");
        metric(
            &mut out,
            "usai_http_upgrades_total",
            "WebSocket upgrades",
            "counter",
            &[(String::new(), h.upgrades as f64)],
        );
        metric(
            &mut out,
            "usai_http_streams_total",
            "Streaming responses",
            "counter",
            &[(String::new(), h.streams as f64)],
        );
    }
    out
}

/// The workload → resource / dispatch graph (`GOAL.md` §40), from the
/// definition. Text form for `usai graph`.
pub fn render_graph(definition: &crate::definition::ApplicationDefinition) -> String {
    use crate::definition::Trigger;
    let mut out = String::new();
    let lifetime = |t: &Trigger| match t {
        Trigger::Http { .. } => "request",
        Trigger::Task => "task",
        Trigger::Cron { .. } => "cron",
        Trigger::Command => "command",
        Trigger::Service { .. } => "service",
        Trigger::Queue { .. } => "message",
        Trigger::Socket { .. } => "connection",
        Trigger::Stream { .. } => "stream",
    };
    let display = |w: &crate::definition::WorkloadSpec| match &w.trigger {
        Trigger::Http { method, path, .. } | Trigger::Stream { method, path } => {
            format!("{method} {path}")
        }
        Trigger::Socket { path } => path.clone(),
        _ => w.name.clone(),
    };
    for w in definition.workloads() {
        let _ = writeln!(out, "{} [{}]", display(w), lifetime(&w.trigger));
        let edges: Vec<String> = w
            .resources
            .iter()
            .map(|r| {
                let kind = definition
                    .resources()
                    .iter()
                    .find(|x| &x.name == r)
                    .map(|x| x.kind.as_str())
                    .unwrap_or("resource");
                // A pool lends a connection; an HTTP client lends a bounded
                // slot for one outbound request.
                let edge = if kind == "http.client" {
                    "egress"
                } else {
                    "lease"
                };
                format!("{kind}/{r} [{edge}]")
            })
            .collect();
        let publishes: Vec<String> = w
            .publishes
            .iter()
            .map(|topic| {
                let target = definition
                    .workload(&format!("queue:{topic}"))
                    .map(|(_, t)| format!("{} [{}]", t.name, lifetime(&t.trigger)))
                    .unwrap_or_else(|| format!("{topic} [message]"));
                format!("publish → {target}")
            })
            .collect();
        let dispatches: Vec<String> = w
            .dispatches
            .iter()
            .map(|d| {
                let target = definition
                    .workload(d)
                    .map(|(_, t)| format!("{} [{}]", t.name, lifetime(&t.trigger)))
                    .unwrap_or_else(|| d.clone());
                format!("dispatch → {target}")
            })
            .collect();
        let all: Vec<String> = edges
            .into_iter()
            .chain(dispatches)
            .chain(publishes)
            .collect();
        for (i, edge) in all.iter().enumerate() {
            let last = i + 1 == all.len();
            let _ = writeln!(out, "   {} {edge}", if last { "└──" } else { "├──" });
        }
        let _ = writeln!(out);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prometheus_text_is_well_formed() {
        let status = RuntimeStatus {
            engine: "quickjs",
            compiled_images_live: 1,
            gauges: crate::ownership::GaugeSnapshot {
                worlds_created: 3,
                live_worlds: 1,
                ..Default::default()
            },
            tasks: serde_json::json!({ "queued": 0, "running": 1, "completed": 2, "failed": 0, "lost": 0 }),
            revisions: vec![],
            resources: vec![],
            worlds_in_use: 1,
            worlds_max: 256,
        };
        let text = render_prometheus(
            &status,
            Some(&HttpSnapshot {
                requests: 10,
                responses_2xx: 9,
                responses_4xx: 1,
                ..Default::default()
            }),
        );
        assert!(text.contains("usai_worlds_live 1"));
        assert!(text.contains("usai_worlds_created_total 3"));
        assert!(text.contains("usai_http_responses_total{class=\"2xx\"} 9"));
        assert!(text.contains("usai_tasks{state=\"running\"} 1"));
        assert!(text.lines().all(|l| l.starts_with('#') || l.contains(' ')));
    }
}
