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

/// One line per key per interval for a failure that repeats on every request
/// while a dependency is down. The first occurrence logs at once; the next
/// line for the same key waits the interval and says how many it stands for.
pub struct RateLimitedLog {
    interval: std::time::Duration,
    seen: std::sync::Mutex<std::collections::BTreeMap<String, (std::time::Instant, u64)>>,
}

impl RateLimitedLog {
    pub const fn new(interval: std::time::Duration) -> Self {
        Self {
            interval,
            seen: std::sync::Mutex::new(std::collections::BTreeMap::new()),
        }
    }

    /// `Some(suppressed)` when the caller should log now (`suppressed` is
    /// how many occurrences since the previous line were not logged),
    /// `None` when this one is folded into the next line.
    pub fn allow(&self, key: &str) -> Option<u64> {
        let now = std::time::Instant::now();
        let mut seen = self.seen.lock().expect("rate limit poisoned");
        match seen.get_mut(key) {
            Some((last, suppressed)) if now.duration_since(*last) < self.interval => {
                *suppressed += 1;
                None
            }
            Some((last, suppressed)) => {
                let n = *suppressed;
                *last = now;
                *suppressed = 0;
                Some(n)
            }
            None => {
                seen.insert(key.to_owned(), (now, 0));
                Some(0)
            }
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
    /// Streams whose handler failed after the head was committed: the client
    /// got a 200 and a body that ends early. Shared with the task that
    /// observes the world after the response left.
    pub streams_failed: std::sync::Arc<AtomicU64>,
    /// Counts per `LATENCY_BUCKETS` entry (non-cumulative), plus overflow.
    pub latency_buckets: [AtomicU64; 15],
    /// Sum of observed latencies, in microseconds.
    pub latency_sum_us: AtomicU64,
    pub rejections: [AtomicU64; 6],
    /// Per-workload response classes and latency. Bounded by the set of
    /// workloads the definitions name, never by request data. Shared,
    /// because a stream's cost is only known when its world ends — long
    /// after the response left the pipeline (`record_into`).
    pub by_workload: WorkloadStats,
}

/// What one workload's responses cost. The classes answer "is this route
/// failing"; the sum and the count answer "is this route slow", which the
/// global histogram cannot: a route that takes 200 ms while 99.7 % of
/// traffic takes 2 ms does not move a p99 at all (measured — an on-call
/// round on 0.0.9 watched the documented latency alert read 2.5 ms while a
/// route took 204 ms). Two extra series per workload, the same order as the
/// per-workload CPU counter.
#[derive(Debug, Default)]
pub struct WorkloadCounters {
    pub classes: [AtomicU64; 4],
    pub latency_sum_us: AtomicU64,
    pub latency_count: AtomicU64,
}

/// One workload's counters, flattened for the status document.
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkloadSnapshot {
    #[serde(rename = "2xx")]
    pub c2xx: u64,
    #[serde(rename = "3xx")]
    pub c3xx: u64,
    #[serde(rename = "4xx")]
    pub c4xx: u64,
    #[serde(rename = "5xx")]
    pub c5xx: u64,
    /// Summed wall time of the responses counted here, and how many were
    /// timed: `latencySumSeconds / count` is this route's mean.
    pub latency_sum_seconds: f64,
    pub count: u64,
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
    pub streams_failed: u64,
    /// Cumulative counts per `LATENCY_BUCKETS` entry, then `+Inf`. Serialised
    /// as `{ "le": [bounds…, "+Inf"], "count": […] }` so the JSON names its
    /// bucket bounds.
    #[serde(serialize_with = "serialize_latency")]
    pub latency_cumulative: Vec<u64>,
    pub latency_sum_seconds: f64,
    /// Rejections by reason: route, validation, auth, capacity, draining,
    /// other — serialised as an object keyed by reason.
    #[serde(serialize_with = "serialize_rejections")]
    pub rejections: [u64; 6],
    /// Per-workload response counts: workload id → [2xx, 3xx, 4xx, 5xx],
    /// serialised as `{ "2xx": n, "3xx": n, "4xx": n, "5xx": n }`.
    pub by_workload: std::collections::BTreeMap<String, WorkloadSnapshot>,
}

pub const REJECTION_REASONS: [&str; 6] = [
    "route",
    "validation",
    "auth",
    "capacity",
    "draining",
    "other",
];

use serde_json::json;

fn serialize_latency<S: serde::Serializer>(v: &[u64], s: S) -> Result<S::Ok, S::Error> {
    use serde::ser::SerializeMap;
    let mut le: Vec<serde_json::Value> = LATENCY_BUCKETS.iter().map(|b| json!(b)).collect();
    le.push(json!("+Inf"));
    let mut m = s.serialize_map(Some(2))?;
    m.serialize_entry("le", &le)?;
    m.serialize_entry("count", v)?;
    m.end()
}

fn serialize_rejections<S: serde::Serializer>(v: &[u64; 6], s: S) -> Result<S::Ok, S::Error> {
    use serde::ser::SerializeMap;
    let mut m = s.serialize_map(Some(6))?;
    for (reason, n) in REJECTION_REASONS.iter().zip(v) {
        m.serialize_entry(reason, n)?;
    }
    m.end()
}

/// The per-workload map, shareable with whoever finishes the work.
pub type WorkloadStats =
    std::sync::Arc<std::sync::RwLock<std::collections::BTreeMap<String, WorkloadCounters>>>;

/// Counts one response under its workload. Free-standing so the task that
/// outlives a streaming response can call it with nothing but the map: a
/// stream's latency is its **world's lifetime**, which is not known until
/// long after the pipeline returned the head.
pub fn record_into(
    by_workload: &WorkloadStats,
    workload: &str,
    status: u16,
    latency: std::time::Duration,
) {
    // 101 (a WebSocket upgrade) is a success, not a server error.
    let class = match status {
        100..=299 => 0,
        300..=399 => 1,
        400..=499 => 2,
        _ => 3,
    };
    let add = |counters: &WorkloadCounters| {
        counters.classes[class].fetch_add(1, Ordering::Relaxed);
        counters
            .latency_sum_us
            .fetch_add(latency.as_micros() as u64, Ordering::Relaxed);
        counters.latency_count.fetch_add(1, Ordering::Relaxed);
    };
    if let Some(counters) = by_workload.read().expect("stats poisoned").get(workload) {
        add(counters);
        return;
    }
    let mut map = by_workload.write().expect("stats poisoned");
    add(map.entry(workload.to_owned()).or_default());
}

impl HttpStats {
    pub fn record(&self, status: u16, before_world: bool) {
        self.record_with(status, before_world, None, None);
    }

    /// Counts a response under the workload that produced (or refused) it,
    /// with what it cost.
    pub fn record_workload(&self, workload: &str, status: u16, latency: std::time::Duration) {
        record_into(&self.by_workload, workload, status, latency);
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
            100..=299 => &self.responses_2xx,
            300..=399 => &self.responses_3xx,
            400..=499 => &self.responses_4xx,
            _ => &self.responses_5xx,
        };
        counter.fetch_add(1, Ordering::Relaxed);
        if status == 101 {
            self.upgrades.fetch_add(1, Ordering::Relaxed);
        }
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
            streams_failed: self.streams_failed.load(Ordering::Relaxed),
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
            by_workload: self
                .by_workload
                .read()
                .expect("stats poisoned")
                .iter()
                .map(|(k, v)| {
                    (
                        k.clone(),
                        WorkloadSnapshot {
                            c2xx: v.classes[0].load(Ordering::Relaxed),
                            c3xx: v.classes[1].load(Ordering::Relaxed),
                            c4xx: v.classes[2].load(Ordering::Relaxed),
                            c5xx: v.classes[3].load(Ordering::Relaxed),
                            latency_sum_seconds: v.latency_sum_us.load(Ordering::Relaxed) as f64
                                / 1e6,
                            count: v.latency_count.load(Ordering::Relaxed),
                        },
                    )
                })
                .collect(),
        }
    }
}

/// Process-level facts for the metrics endpoint: resident set, start time,
/// build info. Linux-only where the kernel exposes them; absent elsewhere.
/// (name, help, kind, samples)
pub type MetricFamily = (&'static str, &'static str, &'static str, Vec<(String, f64)>);

pub fn process_metrics() -> Vec<MetricFamily> {
    let mut out = Vec::new();
    out.push((
        "usai_build_info",
        "Usai runtime version (label), always 1",
        "gauge",
        vec![(
            format!("version=\"{}\"", crate::definition::RUNTIME_VERSION),
            1.0,
        )],
    ));
    static STARTED: std::sync::OnceLock<f64> = std::sync::OnceLock::new();
    let started = *STARTED.get_or_init(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0)
    });
    out.push((
        "usai_process_start_time_seconds",
        "Unix time the runtime started",
        "gauge",
        vec![(String::new(), started)],
    ));
    if let Some(p) = crate::procfs::read() {
        let kib = 1024.0;
        out.push((
            "usai_process_resident_memory_bytes",
            "Resident set size",
            "gauge",
            vec![(String::new(), p.rss_kib as f64 * kib)],
        ));
        out.push((
            "usai_process_proportional_memory_bytes",
            "Proportional set size (shared pages divided among their sharers): the honest per-process footprint",
            "gauge",
            vec![(String::new(), p.pss_kib as f64 * kib)],
        ));
        out.push((
            "usai_process_virtual_memory_bytes",
            "Virtual size (address space reserved per world slot up front; not resident)",
            "gauge",
            vec![(String::new(), p.vm_kib as f64 * kib)],
        ));
        out.push((
            "usai_process_resident_memory_peak_bytes",
            "Peak resident set size since start",
            "gauge",
            vec![(String::new(), p.rss_peak_kib as f64 * kib)],
        ));
        out.push((
            "usai_process_page_faults_total",
            "Page faults since start, by kind",
            "counter",
            vec![
                ("kind=\"minor\"".into(), p.minor_faults as f64),
                ("kind=\"major\"".into(), p.major_faults as f64),
            ],
        ));
        out.push((
            "usai_process_cpu_seconds_total",
            "CPU consumed since start, user + system",
            "counter",
            vec![(String::new(), p.cpu_seconds)],
        ));
        out.push((
            "usai_process_threads",
            "OS threads",
            "gauge",
            vec![(String::new(), p.threads as f64)],
        ));
        out.push((
            "usai_process_open_fds",
            "Open file descriptors",
            "gauge",
            vec![(String::new(), p.open_fds as f64)],
        ));
        // The cgroup's own view, which is the one that decides whether this
        // process lives. Only published when there is a limit: on a host
        // without one these would be four zeroes an alert could misread.
        if p.memory_limit_bytes > 0 {
            out.push((
                "usai_process_memory_limit_bytes",
                "The cgroup memory limit this process runs under",
                "gauge",
                vec![(String::new(), p.memory_limit_bytes as f64)],
            ));
            out.push((
                "usai_process_memory_charged_bytes",
                "What the cgroup is charged (memory.current): resident memory plus the page cache this container faulted in",
                "gauge",
                vec![(String::new(), p.memory_charged_bytes as f64)],
            ));
            out.push((
                "usai_process_memory_ceiling_hits_total",
                "Times the cgroup hit its limit and had to reclaim (memory.events max): climbing means the container is thrashing on its own pages, which is not an OOM kill and is not logged anywhere else",
                "counter",
                vec![(String::new(), p.memory_ceiling_hits as f64)],
            ));
            out.push((
                "usai_process_memory_oom_kills_total",
                "OOM kills inside this cgroup (memory.events oom_kill); normally 0, because a kill of this process takes the metric with it",
                "counter",
                vec![(String::new(), p.memory_oom_kills as f64)],
            ));
        }
    }
    out
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
        request_id = result.request_id.as_deref().unwrap_or(""),
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
        "usai_scheduler",
        "1 when this instance runs the scheduler of that kind (cron: exactly one replica should)",
        "gauge",
        &[
            ("kind=\"cron\"".into(), f64::from(status.scheduler.cron)),
            ("kind=\"queue\"".into(), f64::from(status.scheduler.queue)),
            (
                "kind=\"services\"".into(),
                f64::from(status.scheduler.services),
            ),
        ],
    );
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
    // The same, per workload: CPU is accounted and not scheduled, so the
    // accounting has to answer "which workload is spending it".
    if !g.guest_cpu_ns_by_workload.is_empty() {
        let by_workload: Vec<(String, f64)> = g
            .guest_cpu_ns_by_workload
            .iter()
            .map(|(w, ns)| (format!("workload=\"{}\"", label(w)), *ns as f64 / 1e9))
            .collect();
        metric(
            &mut out,
            "usai_workload_cpu_seconds_total",
            "Thread CPU time spent executing guest code, per workload",
            "counter",
            &by_workload,
        );
    }
    // The same for the worlds themselves. A stream or a socket holds a
    // world for as long as its connection lives and moves no per-request
    // rate while it does, so the total alone cannot say which workload is
    // holding the budget.
    let live_by_workload: Vec<(String, f64)> = status
        .revisions
        .iter()
        .flat_map(|r| r.live_by_workload.iter())
        .fold(
            std::collections::BTreeMap::<&str, u64>::new(),
            |mut acc, (w, n)| {
                *acc.entry(w.as_str()).or_default() += n;
                acc
            },
        )
        .into_iter()
        .map(|(w, n)| (format!("workload=\"{}\"", label(w)), n as f64))
        .collect();
    if !live_by_workload.is_empty() {
        metric(
            &mut out,
            "usai_workload_worlds_live",
            "Execution worlds currently alive, per workload",
            "gauge",
            &live_by_workload,
        );
    }
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
        "usai_deadline_unwind_overruns_total",
        "Worlds cancelled because they did not unwind within the deadline grace",
        "counter",
        &[(String::new(), g.deadline_unwind_overruns as f64)],
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
                    "revision=\"{}\",application=\"{}\",state=\"{}\"",
                    r.id,
                    label(&r.application),
                    format!("{:?}", r.state).to_lowercase()
                ),
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
                        "revision=\"{}\",service=\"{}\",state=\"{}\"",
                        r.id,
                        label(&s.name),
                        format!("{:?}", s.state).to_lowercase()
                    ),
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
                ("reclaimed", r.queue.reclaimed),
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
    // The same, per topic. An alert on dead letters that cannot name the
    // queue sends whoever it woke looking through every topic the
    // application has — and the line that would have said which was, until
    // this release, not written at all for a contract failure.
    let by_topic: Vec<(String, f64)> = status
        .revisions
        .iter()
        .flat_map(|r| {
            r.queue_by_topic.iter().flat_map(move |t| {
                [
                    ("claimed", t.claimed),
                    ("done", t.done),
                    ("retried", t.retried),
                    ("dead", t.dead),
                    ("invalid", t.invalid),
                    ("reclaimed", t.reclaimed),
                ]
                .into_iter()
                .map(move |(state, n)| {
                    (
                        format!(
                            "revision=\"{}\",topic=\"{}\",state=\"{state}\"",
                            r.id,
                            label(&t.topic)
                        ),
                        n as f64,
                    )
                })
            })
        })
        .collect();
    if !by_topic.is_empty() {
        metric(
            &mut out,
            "usai_queue_topic_messages_total",
            "Queue messages by outcome, per topic",
            "counter",
            &by_topic,
        );
    }
    let cron: Vec<(String, f64)> = status
        .revisions
        .iter()
        .flat_map(|r| {
            [
                ("due", r.cron.ticks),
                ("skipped", r.cron.skipped),
                ("failed", r.cron.failed),
                ("taken", r.cron.taken),
            ]
            .into_iter()
            .map(move |(state, n)| (format!("revision=\"{}\",state=\"{state}\"", r.id), n as f64))
        })
        .collect();
    if !cron.is_empty() {
        metric(
            &mut out,
            "usai_cron_ticks_total",
            "Cron ticks per revision: due on this instance, skipped (previous still running), failed, taken by another instance (exclusive schedules)",
            "counter",
            &cron,
        );
    }
    // Levels (gauge) and events (counter) are different metrics: the
    // number of quarantines is cumulative, so it is not a `usai_resource`
    // level and must not be alerted on as one.
    let mut resources = Vec::new();
    let mut quarantines = Vec::new();
    let (mut operations, mut transactions) = (Vec::new(), Vec::new());
    let (mut outbound, mut outbound_failures, mut outbound_refused) =
        (Vec::new(), Vec::new(), Vec::new());
    for r in &status.resources {
        let base = format!(
            "kind=\"{}\",name=\"{}\"",
            label(&r.identity.kind),
            label(&r.identity.name)
        );
        resources.push((format!("{base},metric=\"in_use\""), r.in_use as f64));
        resources.push((format!("{base},metric=\"max\""), r.max as f64));
        // What the last contact said (1 = reachable), so an outage is a
        // gauge to alert on, not only a readiness probe's answer.
        resources.push((
            format!("{base},metric=\"ready\""),
            if r.ready { 1.0 } else { 0.0 },
        ));
        // The one number that separates "the dependency is slow" from "my
        // pool is too small", which are opposite fixes: `in_use == max` is
        // healthy saturation, `waiting > 0` is a queue. It was in the status
        // document and nowhere else, so an on-call round spent twenty-five
        // minutes on the difference (`docs/runbooks/slow-route.md`).
        // A level, like in_use and max.
        if let Some(waiting) = r.detail.get("waiting").and_then(|v| v.as_u64()) {
            resources.push((format!("{base},metric=\"waiting\""), waiting as f64));
        }
        // Events are counters, per this page's own convention — and they
        // were in the status document only, so outbound failure rate could
        // not be graphed at all.
        for (key, into) in [
            ("operations", &mut operations),
            ("transactions", &mut transactions),
            ("requests", &mut outbound),
            ("failures", &mut outbound_failures),
            ("refused", &mut outbound_refused),
        ] {
            if let Some(value) = r.detail.get(key).and_then(|v| v.as_u64()) {
                into.push((base.clone(), value as f64));
            }
        }
        quarantines.push((base, r.quarantined as f64));
    }
    if !resources.is_empty() {
        metric(
            &mut out,
            "usai_resource",
            "Resource manager state (current levels)",
            "gauge",
            &resources,
        );
        for (name, help, values) in [
            (
                "usai_resource_operations_total",
                "Operations leased from this resource (cumulative)",
                &operations,
            ),
            (
                "usai_resource_transactions_total",
                "Transactions opened on this resource (cumulative)",
                &transactions,
            ),
            (
                "usai_resource_requests_total",
                "Outbound requests made through this resource (cumulative)",
                &outbound,
            ),
            (
                "usai_resource_failures_total",
                "Outbound requests that failed (cumulative): the rate an outbound dependency is failing at",
                &outbound_failures,
            ),
            (
                "usai_resource_refused_total",
                "Outbound requests refused before they were made, by the resource's own bound (cumulative)",
                &outbound_refused,
            ),
        ] {
            if !values.is_empty() {
                metric(&mut out, name, help, "counter", values);
            }
        }
        metric(
            &mut out,
            "usai_resource_quarantines_total",
            "Connections quarantined because their outcome could not be proven (cumulative)",
            "counter",
            &quarantines,
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
    for (name, help, kind, samples) in process_metrics() {
        metric(&mut out, name, help, kind, &samples);
    }
    if let Some(h) = http {
        let by_workload: Vec<(String, f64)> = h
            .by_workload
            .iter()
            .flat_map(|(w, counts)| {
                [
                    ("2xx", counts.c2xx),
                    ("3xx", counts.c3xx),
                    ("4xx", counts.c4xx),
                    ("5xx", counts.c5xx),
                ]
                .into_iter()
                .map(move |(class, n)| {
                    (
                        format!("workload=\"{}\",class=\"{class}\"", label(w)),
                        n as f64,
                    )
                })
            })
            .collect();
        if !by_workload.is_empty() {
            metric(
                &mut out,
                "usai_http_workload_responses_total",
                "HTTP responses by workload and status class (refusals after routing included)",
                "counter",
                &by_workload,
            );
            // Per-workload latency as a summary's two halves. The global
            // histogram cannot find a slow route: one that takes 200 ms while
            // 99.7 % of traffic takes 2 ms leaves the p99 at 2.5 ms
            // (measured). `rate(sum) / rate(count)` by workload does, in one
            // query, at two series per workload.
            let sums: Vec<(String, f64)> = h
                .by_workload
                .iter()
                .map(|(w, c)| (format!("workload=\"{}\"", label(w)), c.latency_sum_seconds))
                .collect();
            metric(
                &mut out,
                "usai_http_workload_request_seconds_sum",
                "Summed response time per workload; divide by _count for the mean, which is how you find a slow route (the global histogram hides one that is a minority of traffic)",
                "counter",
                &sums,
            );
            let counts: Vec<(String, f64)> = h
                .by_workload
                .iter()
                .map(|(w, c)| (format!("workload=\"{}\"", label(w)), c.count as f64))
                .collect();
            metric(
                &mut out,
                "usai_http_workload_request_seconds_count",
                "Responses timed per workload (the denominator of _sum)",
                "counter",
                &counts,
            );
        }
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
            &REJECTION_REASONS
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
        metric(
            &mut out,
            "usai_http_streams_failed_total",
            "Streaming responses whose handler failed after the head was sent (the client saw a 200 and a body that ended early)",
            "counter",
            &[(String::new(), h.streams_failed as f64)],
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
        Trigger::Http { method, path, .. } | Trigger::Stream { method, path, .. } => {
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
                format!("hands work to → {target}")
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
    // What the graph is drawn from, so a missing edge reads as a missing
    // declaration rather than as an application that hands off nothing.
    let _ = writeln!(
        out,
        "Edges are declarations: resources from a workload's `resources: [...]`, hand-offs from\n\
         `dispatches(<workload>, <task>)` and topics from `publishes(...)`, all at module scope.\n\
         A hand-off the application performs without declaring it runs, and is logged, but cannot\n\
         appear here."
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prometheus_text_is_well_formed() {
        let status = RuntimeStatus {
            engine: "quickjs",
            scheduler: crate::runtime::SchedulerStatus {
                cron: true,
                queue: true,
                services: true,
            },
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
            process: None,
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

    #[test]
    fn a_rate_limited_log_folds_repeats_into_the_next_line() {
        let log = RateLimitedLog::new(std::time::Duration::from_millis(50));
        assert_eq!(
            log.allow("pool_error"),
            Some(0),
            "the first occurrence logs at once"
        );
        assert_eq!(log.allow("pool_error"), None);
        assert_eq!(log.allow("pool_error"), None);
        assert_eq!(
            log.allow("connection_closed"),
            Some(0),
            "keys are independent"
        );
        std::thread::sleep(std::time::Duration::from_millis(60));
        assert_eq!(
            log.allow("pool_error"),
            Some(2),
            "the next line says what it stands for"
        );
        assert_eq!(log.allow("pool_error"), None);
    }
}
