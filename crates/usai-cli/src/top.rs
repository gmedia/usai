//! `usai top`: what a running instance is doing **now**.
//!
//! `/_usai/status` is cumulative — it answers "how many requests since the
//! process started", which is the wrong question during an incident. This
//! takes two samples and reports the difference: requests per second per
//! workload, the average and the CPU share of each one, what the pool is
//! doing, and what the process costs. Nothing here is a new measurement
//! surface; it is the status document, differenced and laid out.

use std::collections::BTreeMap;
use std::time::Duration;

use anyhow::{Context, Result};
use serde_json::Value;

/// One sample, reduced to the numbers the screen uses.
struct Sample {
    at: std::time::Instant,
    raw: Value,
}

impl Sample {
    fn get(&self, path: &[&str]) -> Option<&Value> {
        let mut cursor = &self.raw;
        for key in path {
            cursor = cursor.get(key)?;
        }
        Some(cursor)
    }

    fn f64(&self, path: &[&str]) -> f64 {
        self.get(path).and_then(Value::as_f64).unwrap_or(0.0)
    }

    fn u64(&self, path: &[&str]) -> u64 {
        self.get(path).and_then(Value::as_u64).unwrap_or(0)
    }

    fn str(&self, path: &[&str]) -> &str {
        self.get(path).and_then(Value::as_str).unwrap_or("")
    }
}

pub async fn run(
    addr: &str,
    interval: f64,
    iterations: Option<u64>,
    token: Option<String>,
) -> Result<()> {
    let interval = interval.clamp(0.2, 3600.0);
    let url = format!("http://{addr}/_usai/status");
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs_f64((interval * 2.0).clamp(2.0, 10.0)))
        .build()?;
    let fetch = async |client: &reqwest::Client| -> Result<Sample> {
        let mut request = client.get(&url);
        if let Some(token) = &token {
            request = request.bearer_auth(token);
        }
        let response = request
            .send()
            .await
            .with_context(|| format!("{url}: no answer"))?;
        let status = response.status();
        if status == reqwest::StatusCode::UNAUTHORIZED {
            anyhow::bail!(
                "{url}: 401 — this instance has a status token; pass --status-token (USAI_STATUS_TOKEN)"
            );
        }
        if status == reqwest::StatusCode::NOT_FOUND {
            anyhow::bail!(
                "{url}: 404 — the status surface is off here (--surfaces-off status, or the instance serves /_usai/* on another listener: --addr)"
            );
        }
        if !status.is_success() {
            anyhow::bail!("{url}: {status}");
        }
        Ok(Sample {
            at: std::time::Instant::now(),
            raw: response.json().await.context("status is not JSON")?,
        })
    };

    let clear = iterations != Some(1) && std::io::IsTerminal::is_terminal(&std::io::stdout());
    let mut previous = fetch(&client).await?;
    let mut drawn = 0u64;
    loop {
        tokio::time::sleep(Duration::from_secs_f64(interval)).await;
        let current = fetch(&client).await?;
        if clear {
            print!("\x1b[2J\x1b[H");
        }
        print!("{}", render(&previous, &current));
        use std::io::Write;
        let _ = std::io::stdout().flush();
        previous = current;
        drawn += 1;
        if iterations.is_some_and(|n| drawn >= n) {
            return Ok(());
        }
    }
}

fn rate(now: u64, before: u64, seconds: f64) -> f64 {
    if seconds <= 0.0 {
        return 0.0;
    }
    (now.saturating_sub(before)) as f64 / seconds
}

/// Milliseconds, with the precision that is actually meaningful at that size.
fn ms(value: f64) -> String {
    if value >= 100.0 {
        format!("{value:.0}ms")
    } else if value >= 10.0 {
        format!("{value:.1}ms")
    } else {
        format!("{value:.2}ms")
    }
}

fn render(before: &Sample, now: &Sample) -> String {
    let seconds = now.at.duration_since(before.at).as_secs_f64();
    let mut out = String::new();

    // Header: which application, which revision, on what substrate.
    let active = now
        .get(&["revisions"])
        .and_then(Value::as_array)
        .and_then(|revs| {
            revs.iter()
                .find(|r| r.get("state").and_then(Value::as_str) == Some("active"))
                .or_else(|| revs.last())
        });
    let (application, revision, state, in_flight) = match active {
        Some(r) => (
            r.get("application").and_then(Value::as_str).unwrap_or("?"),
            r.get("id").and_then(Value::as_u64).unwrap_or(0),
            r.get("state").and_then(Value::as_str).unwrap_or("?"),
            r.get("inFlight").and_then(Value::as_u64).unwrap_or(0),
        ),
        None => ("(no revision)", 0, "-", 0),
    };
    out.push_str(&format!(
        "{application}  revision {revision} {state}  engine {}  every {seconds:.1}s\n\n",
        now.str(&["engine"])
    ));

    // What the process is spending.
    // A restarted instance's counters go backwards; a negative rate would be
    // a lie about the new process, so every difference is floored at zero.
    let cpu_percent =
        (now.f64(&["process", "cpuSeconds"]) - before.f64(&["process", "cpuSeconds"])).max(0.0)
            / seconds
            * 100.0;
    out.push_str(&format!(
        "  worlds {}/{} live, {} in flight   tasks {} running, {} queued (max {})\n",
        now.u64(&["worldsInUse"]),
        now.u64(&["worldsMax"]),
        in_flight,
        now.u64(&["tasks", "running"]),
        now.u64(&["tasks", "queued"]),
        now.u64(&["tasks", "max"]),
    ));
    // Three memory numbers that disagree, and the one to act on depends on
    // the question. RSS counts the pooled Wasm image once per slot it is
    // mapped into, so it overstates; PSS divides that sharing out; and under
    // a limit the **cgroup's** charge is the number that gets the process
    // killed, so that is what leads when there is one.
    let limit = now.f64(&["process", "memoryLimitBytes"]);
    let memory = if limit > 0.0 {
        format!(
            "mem {:.1}/{:.0} MiB charged (rss {:.1}, pss {:.1})",
            now.f64(&["process", "memoryChargedBytes"]) / 1_048_576.0,
            limit / 1_048_576.0,
            now.f64(&["process", "rssKib"]) / 1024.0,
            now.f64(&["process", "pssKib"]) / 1024.0,
        )
    } else {
        format!(
            "mem rss {:.1} MiB (peak {:.1}), pss {:.1}",
            now.f64(&["process", "rssKib"]) / 1024.0,
            now.f64(&["process", "rssPeakKib"]) / 1024.0,
            now.f64(&["process", "pssKib"]) / 1024.0,
        )
    };
    out.push_str(&format!(
        "  {memory}   cpu {cpu_percent:.0}%   fds {}   threads {}   worlds/s {:.0}\n",
        now.u64(&["process", "openFds"]),
        now.u64(&["process", "threads"]),
        rate(
            now.u64(&["gauges", "worldsCreated"]),
            before.u64(&["gauges", "worldsCreated"]),
            seconds
        ),
    ));
    // Reclaim is how a box under a limit fails *without* being killed: it is
    // charged to its ceiling and spends its time faulting its own text back
    // in. Absence of an OOM kill proves nothing, so both are on the screen.
    let ceiling = now
        .u64(&["process", "memoryCeilingHits"])
        .saturating_sub(before.u64(&["process", "memoryCeilingHits"]));
    let killed = now.u64(&["process", "memoryOomKills"]);
    if ceiling > 0 || killed > 0 {
        out.push_str(&format!(
            "  AT THE MEMORY CEILING: {ceiling} reclaim events in this window{}\n",
            if killed > 0 {
                format!(", {killed} OOM kill(s) since boot")
            } else {
                String::new()
            }
        ));
    }

    // Worlds alive per workload, summed over revisions. A stream or a
    // socket holds one for as long as its connection lives and completes no
    // requests while it does, so without this column the runaway export is
    // a row of zeros — which is what an incident screen showed before.
    let mut live: BTreeMap<String, u64> = BTreeMap::new();
    if let Some(revisions) = now.get(&["revisions"]).and_then(Value::as_array) {
        for revision in revisions {
            if let Some(map) = revision.get("liveByWorkload").and_then(Value::as_object) {
                for (workload, n) in map {
                    *live.entry(workload.clone()).or_default() += n.as_u64().unwrap_or(0);
                }
            }
        }
    }

    // Per workload: the table the global histogram cannot give you.
    let empty = serde_json::Map::new();
    let by_workload = now
        .get(&["http", "by_workload"])
        .and_then(Value::as_object)
        .unwrap_or(&empty);
    let mut rows: Vec<(String, f64, u64, u64, u64, f64, f64)> = Vec::new();
    for (workload, stats) in by_workload {
        let count = stats.get("count").and_then(Value::as_u64).unwrap_or(0);
        let was = before
            .get(&["http", "by_workload", workload.as_str()])
            .cloned()
            .unwrap_or(Value::Null);
        let count_before = was.get("count").and_then(Value::as_u64).unwrap_or(0);
        let served = count.saturating_sub(count_before);
        let class = |key: &str| {
            let now = stats.get(key).and_then(Value::as_u64).unwrap_or(0);
            let then = was.get(key).and_then(Value::as_u64).unwrap_or(0);
            now.saturating_sub(then)
        };
        let latency = (stats
            .get("latencySumSeconds")
            .and_then(Value::as_f64)
            .unwrap_or(0.0)
            - was
                .get("latencySumSeconds")
                .and_then(Value::as_f64)
                .unwrap_or(0.0))
        .max(0.0);
        let cpu_ns = (now.f64(&["gauges", "guestCpuNsByWorkload", workload.as_str()])
            - before.f64(&["gauges", "guestCpuNsByWorkload", workload.as_str()]))
        .max(0.0);
        // An idle workload is still worth a row: "that route served nothing
        // this second" is an answer during an incident.
        rows.push((
            workload.clone(),
            served as f64 / seconds,
            live.remove(workload.as_str()).unwrap_or(0),
            class("4xx"),
            class("5xx"),
            if served > 0 {
                latency / served as f64 * 1000.0
            } else {
                0.0
            },
            if served > 0 {
                cpu_ns / served as f64 / 1_000_000.0
            } else {
                0.0
            },
        ));
    }
    // A workload that has live worlds and has completed nothing yet — the
    // first long export, a socket that just connected — has no row in the
    // response table at all. It is exactly the one to show.
    for (workload, n) in live {
        rows.push((workload, 0.0, n, 0, 0, 0.0, 0.0));
    }
    rows.sort_by(|a, b| {
        b.1.total_cmp(&a.1)
            .then_with(|| b.2.cmp(&a.2))
            .then_with(|| a.0.cmp(&b.0))
    });
    let width = rows
        .iter()
        .map(|r| r.0.len())
        .max()
        .unwrap_or(8)
        .clamp(8, 44);
    out.push_str(&format!(
        "\n  {:<width$}  {:>8}  {:>5}  {:>5}  {:>5}  {:>9}  {:>9}\n",
        "workload", "req/s", "live", "4xx", "5xx", "avg", "cpu"
    ));
    if rows.is_empty() {
        out.push_str("  (no HTTP workload has been called yet)\n");
    }
    for (workload, per_second, alive, s4xx, s5xx, avg, cpu) in rows.iter().take(20) {
        let short = if workload.chars().count() > width {
            let tail: String = workload
                .chars()
                .skip(workload.chars().count() - width + 1)
                .collect();
            format!("…{tail}")
        } else {
            workload.clone()
        };
        out.push_str(&format!(
            "  {short:<width$}  {per_second:>8.1}  {alive:>5}  {s4xx:>5}  {s5xx:>5}  {:>9}  {:>9}\n",
            ms(*avg),
            ms(*cpu)
        ));
    }

    // Rejections never reach a workload, so they are nowhere in the table
    // above — and "the route is fine, the requests are being refused" is
    // exactly the case that wastes an hour.
    let mut refused: Vec<String> = Vec::new();
    for kind in [
        "route",
        "validation",
        "auth",
        "capacity",
        "draining",
        "other",
    ] {
        let delta = rate(
            now.u64(&["http", "rejections", kind]),
            before.u64(&["http", "rejections", kind]),
            seconds,
        );
        if delta > 0.0 {
            refused.push(format!("{kind} {delta:.1}/s"));
        }
    }
    if !refused.is_empty() {
        out.push_str(&format!(
            "  rejected before a world: {}\n",
            refused.join(", ")
        ));
    }

    // Resources: `waiting` is the field that separates "the dependency is
    // slow" from "my pool is too small".
    let resources = now
        .get(&["resources"])
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if !resources.is_empty() {
        let previous: BTreeMap<&str, &Value> = before
            .get(&["resources"])
            .and_then(Value::as_array)
            .map(|list| {
                list.iter()
                    .filter_map(|r| Some((r.get("identity")?.get("name")?.as_str()?, r)))
                    .collect()
            })
            .unwrap_or_default();
        out.push_str(&format!(
            "\n  {:<20}  {:>9}  {:>7}  {:>9}  {:>9}\n",
            "resource", "in use", "waiting", "ops/s", "state"
        ));
        for resource in &resources {
            let name = resource
                .get("identity")
                .and_then(|i| i.get("name"))
                .and_then(Value::as_str)
                .unwrap_or("?");
            let kind = resource
                .get("identity")
                .and_then(|i| i.get("kind"))
                .and_then(Value::as_str)
                .unwrap_or("?");
            let counter = |value: Option<&Value>| -> u64 {
                let detail = value.and_then(|r| r.get("detail"));
                detail
                    .and_then(|d| d.get("operations").or_else(|| d.get("requests")))
                    .and_then(Value::as_u64)
                    .unwrap_or(0)
            };
            let ops = rate(
                counter(Some(resource)),
                counter(previous.get(name).copied()),
                seconds,
            );
            let ready = resource
                .get("ready")
                .and_then(Value::as_bool)
                .unwrap_or(true);
            let waiting = resource
                .get("detail")
                .and_then(|d| d.get("waiting"))
                .and_then(Value::as_u64)
                .unwrap_or(0);
            out.push_str(&format!(
                "  {:<20}  {:>9}  {:>7}  {ops:>9.1}  {:>9}\n",
                format!("{name} ({kind})"),
                format!(
                    "{}/{}",
                    resource.get("in_use").and_then(Value::as_u64).unwrap_or(0),
                    resource.get("max").and_then(Value::as_u64).unwrap_or(0)
                ),
                waiting,
                if ready { "ready" } else { "UNREADY" },
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample(seconds_ago: u64, raw: Value) -> Sample {
        Sample {
            at: std::time::Instant::now() - Duration::from_secs(seconds_ago),
            raw,
        }
    }

    fn status(requests: u64, latency_sum: f64, cpu_ns: u64, cpu_seconds: f64) -> Value {
        json!({
            "engine": "wasm",
            "worldsInUse": 3,
            "worldsMax": 256,
            "tasks": { "running": 1, "queued": 2, "max": 64 },
            "process": { "rssKib": 102400, "rssPeakKib": 112640, "pssKib": 71680, "cpuSeconds": cpu_seconds, "openFds": 14, "threads": 20 },
            "gauges": {
                "worldsCreated": requests,
                "guestCpuNsByWorkload": { "http:GET /hello/:name": cpu_ns },
            },
            "http": {
                "by_workload": {
                    "http:GET /hello/:name": {
                        "count": requests, "2xx": requests, "3xx": 0, "4xx": 0, "5xx": 0,
                        "latencySumSeconds": latency_sum,
                    }
                },
                "rejections": { "route": 0, "validation": 4, "auth": 0, "capacity": 0, "draining": 0, "other": 0 },
            },
            "revisions": [{ "application": "hello", "id": 1, "state": "active", "inFlight": 0 }],
            "resources": [{
                "identity": { "kind": "postgres", "name": "db", "fingerprint": "x", "compat": 1 },
                "ready": true, "in_use": 2, "max": 16, "quarantined": 0,
                "detail": { "operations": 100, "waiting": 5 },
            }],
        })
    }

    /// The whole point of the verb: totals since boot answer the wrong
    /// question, so every number on the screen is a difference over the
    /// window — and the averages divide by the requests *in that window*,
    /// not by all of them.
    #[test]
    fn every_number_is_a_difference_over_the_window() {
        let before = sample(2, status(1_000, 10.0, 1_000_000_000, 5.0));
        // 200 requests in 2 s, 0.5 s of latency and 0.2 s of guest CPU
        // between them: 100 req/s, a 2.5 ms average, 1 ms of CPU each.
        let now = sample(0, status(1_200, 10.5, 1_200_000_000, 5.2));
        let screen = render(&before, &now);
        assert!(screen.contains("hello  revision 1 active"), "{screen}");
        assert!(screen.contains("100.0"), "req/s: {screen}");
        assert!(screen.contains("2.50ms"), "average: {screen}");
        assert!(screen.contains("1.00ms"), "cpu: {screen}");
        // 0.2 CPU-seconds over a 2 s window is 10 % of one core.
        assert!(screen.contains("cpu 10%"), "{screen}");
        // A pool that is queueing says so; the resource row carries it.
        assert!(screen.contains("db (postgres)"), "{screen}");
        assert!(screen.contains("2/16"), "{screen}");
    }

    /// A rejection never reaches a workload, so it is in no row of the
    /// table — and "the route is fine, the requests are being refused" is
    /// the case that wastes an hour. It gets its own line, but only when
    /// there is one.
    #[test]
    fn rejections_are_shown_only_when_they_happen() {
        let quiet = render(
            &sample(2, status(1_000, 10.0, 1_000_000_000, 5.0)),
            &sample(0, status(1_200, 10.5, 1_200_000_000, 5.2)),
        );
        assert!(!quiet.contains("rejected before a world"), "{quiet}");

        let before = status(1_000, 10.0, 1_000_000_000, 5.0);
        let mut after = status(1_200, 10.5, 1_200_000_000, 5.2);
        after["http"]["rejections"]["validation"] = json!(24);
        let loud = render(&sample(2, before), &sample(0, after));
        assert!(loud.contains("rejected before a world"), "{loud}");
        assert!(loud.contains("validation 10.0/s"), "{loud}");
    }

    /// RSS counts the pooled Wasm image once per slot it is mapped into, so
    /// under a limit the number to act on is the **cgroup's** charge — the
    /// one that gets the process killed. Without a limit there is no such
    /// number and the screen says RSS and PSS.
    #[test]
    fn the_memory_number_that_leads_is_the_one_that_kills_you() {
        let free = render(
            &sample(2, status(1_000, 10.0, 1_000_000_000, 5.0)),
            &sample(0, status(1_200, 10.5, 1_200_000_000, 5.2)),
        );
        assert!(
            free.contains("mem rss 100.0 MiB (peak 110.0), pss 70.0"),
            "{free}"
        );
        assert!(!free.contains("charged"), "{free}");
        assert!(!free.contains("MEMORY CEILING"), "{free}");

        let mut before = status(1_000, 10.0, 1_000_000_000, 5.0);
        before["process"]["memoryLimitBytes"] = json!(201_326_592u64);
        before["process"]["memoryChargedBytes"] = json!(88_080_384u64);
        before["process"]["memoryCeilingHits"] = json!(4);
        let mut after = before.clone();
        after["process"]["memoryChargedBytes"] = json!(94_371_840u64);
        after["process"]["memoryCeilingHits"] = json!(1_204);
        after["process"]["memoryOomKills"] = json!(1);
        let limited = render(&sample(2, before), &sample(0, after));
        assert!(limited.contains("mem 90.0/192 MiB charged"), "{limited}");
        assert!(limited.contains("rss 100.0, pss 70.0"), "{limited}");
        // 1 200 reclaim events in two seconds is a box thrashing on its own
        // text; it is not being killed, which is the failure that hides.
        assert!(
            limited.contains("AT THE MEMORY CEILING: 1200 reclaim events"),
            "{limited}"
        );
        assert!(limited.contains("1 OOM kill(s) since boot"), "{limited}");
    }

    /// A stream or a socket holds a world for as long as its connection
    /// lives and completes nothing while it does. Without a `live` column
    /// the incident screen shows the runaway export as a row of zeros — or,
    /// on its first run, as no row at all.
    #[test]
    fn a_workload_that_is_running_shows_even_when_it_has_finished_nothing() {
        let mut before = status(1_000, 10.0, 1_000_000_000, 5.0);
        before["revisions"][0]["liveByWorkload"] = json!({ "stream:GET /exports/invoices.csv": 2 });
        let mut after = status(1_200, 10.5, 1_200_000_000, 5.2);
        after["revisions"][0]["liveByWorkload"] =
            json!({ "stream:GET /exports/invoices.csv": 3, "http:GET /hello/:name": 1 });
        let screen = render(&sample(2, before), &sample(0, after));
        // The stream has no row in by_workload at all — it has completed
        // nothing — and it is still on the screen, with its three worlds.
        let row = screen
            .lines()
            .find(|l| l.contains("stream:GET /exports/invoices.csv"))
            .unwrap_or_else(|| panic!("no row for a live stream:\n{screen}"));
        assert!(row.split_whitespace().any(|f| f == "3"), "{row}");
        // And a workload that is both serving and running shows both.
        let served = screen
            .lines()
            .find(|l| l.contains("http:GET /hello/:name"))
            .unwrap_or_else(|| panic!("no row for the served workload:\n{screen}"));
        assert!(served.contains("100.0"), "{served}");
        assert!(served.split_whitespace().any(|f| f == "1"), "{served}");
    }

    /// A restarted instance's counters go backwards. That is a fact about
    /// the process, not a reason to print a negative rate.
    #[test]
    fn a_counter_that_went_backwards_reads_zero() {
        let screen = render(
            &sample(2, status(5_000, 50.0, 5_000_000_000, 50.0)),
            &sample(0, status(10, 0.1, 10_000_000, 0.1)),
        );
        assert!(!screen.contains('-'), "a negative rate: {screen}");
    }
}
