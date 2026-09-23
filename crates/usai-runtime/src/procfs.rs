//! What the process holds, read from `/proc/self` when someone asks
//! (`/_usai/status`, `/_usai/metrics`): nothing is sampled in the
//! background. Linux only; `None` elsewhere.
//!
//! PSS (proportional set size) is the honest memory number for a runtime
//! whose worlds share copy-on-write pages of one image: RSS counts a shared
//! page once per mapping, PSS divides it among its sharers.

use serde::Serialize;

#[derive(Clone, Debug, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProcessStatus {
    /// Resident set size (`VmRSS`), KiB.
    pub rss_kib: u64,
    /// Proportional set size (`smaps_rollup` `Pss`), KiB; 0 when the kernel
    /// does not expose it.
    pub pss_kib: u64,
    /// Virtual size (`VmSize`), KiB — large by design: the pooling allocator
    /// reserves address space per world slot up front.
    pub vm_kib: u64,
    /// Peak resident set (`VmHWM`), KiB.
    pub rss_peak_kib: u64,
    /// Minor and major page faults since start (`/proc/self/stat`).
    pub minor_faults: u64,
    pub major_faults: u64,
    /// CPU consumed since start, user + system, in seconds.
    pub cpu_seconds: f64,
    pub threads: u64,
    pub open_fds: u64,
    /// The memory limit this process runs under (cgroup v2 `memory.max`),
    /// bytes; 0 when there is none or the file is unreadable.
    pub memory_limit_bytes: u64,
    /// What the cgroup is charged right now (`memory.current`), bytes.
    pub memory_charged_bytes: u64,
    /// How many times the cgroup has hit its limit and had to reclaim
    /// (`memory.events` `max`). **This is the number that identifies the
    /// state between "fits" and "OOM-killed"**: a container pinned at its
    /// ceiling reclaims the clean pages of the binary it is executing and
    /// faults them straight back in, so nothing is killed, nothing is
    /// logged, and throughput collapses. Measured at 1.5 M hits on a
    /// deliberately undersized box that served one request per second
    /// (`docs/runbooks/memory-pressure.md`).
    pub memory_ceiling_hits: u64,
    /// OOM kills inside this cgroup (`memory.events` `oom_kill`) — normally
    /// 0, because the kill takes this process with it; non-zero means a
    /// *child* was killed.
    pub memory_oom_kills: u64,
}

#[cfg(target_os = "linux")]
pub fn read() -> Option<ProcessStatus> {
    let mut out = ProcessStatus::default();
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    for line in status.lines() {
        let mut parts = line.split_whitespace();
        let (Some(key), Some(value)) = (parts.next(), parts.next()) else {
            continue;
        };
        let value: u64 = value.parse().unwrap_or(0);
        match key {
            "VmRSS:" => out.rss_kib = value,
            "VmSize:" => out.vm_kib = value,
            "VmHWM:" => out.rss_peak_kib = value,
            "Threads:" => out.threads = value,
            _ => {}
        }
    }
    if let Ok(rollup) = std::fs::read_to_string("/proc/self/smaps_rollup") {
        out.pss_kib = rollup
            .lines()
            .find_map(|l| l.strip_prefix("Pss:"))
            .and_then(|v| v.split_whitespace().next())
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
    }
    if let Ok(stat) = std::fs::read_to_string("/proc/self/stat") {
        // Fields after the parenthesised command name: (3) state … (10) minflt
        // (12) majflt (14) utime (15) stime, 1-based over the whole line.
        if let Some(rest) = stat.rsplit_once(')').map(|(_, r)| r) {
            let fields: Vec<&str> = rest.split_whitespace().collect();
            // `fields[0]` is field 3 (state).
            let field = |n: usize| fields.get(n - 3).and_then(|v| v.parse::<u64>().ok());
            out.minor_faults = field(10).unwrap_or(0);
            out.major_faults = field(12).unwrap_or(0);
            let ticks = field(14).unwrap_or(0) + field(15).unwrap_or(0);
            let hz = clock_ticks_per_second();
            out.cpu_seconds = ticks as f64 / hz as f64;
        }
    }
    out.open_fds = std::fs::read_dir("/proc/self/fd")
        .map(|d| d.count() as u64)
        .unwrap_or(0);
    read_cgroup(&mut out);
    Some(out)
}

/// The cgroup this process lives in, read the same way anything else here is:
/// when someone asks, never in the background. Only cgroup v2 (`0::<path>` in
/// `/proc/self/cgroup`); on v1, or outside a cgroup, the fields stay 0.
#[cfg(target_os = "linux")]
fn read_cgroup(out: &mut ProcessStatus) {
    let Ok(own) = std::fs::read_to_string("/proc/self/cgroup") else {
        return;
    };
    let Some(rel) = own
        .lines()
        .find_map(|l| l.strip_prefix("0::").map(str::trim))
    else {
        return;
    };
    let dir = format!("/sys/fs/cgroup{rel}");
    let number = |file: &str| -> u64 {
        std::fs::read_to_string(format!("{dir}/{file}"))
            .ok()
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(0)
    };
    // "max" (no limit) parses as 0, and with no limit none of this means
    // anything about *this* process: the charge on an unlimited slice is the
    // whole slice's, which read as a per-process number is nonsense. Report
    // the set or nothing.
    out.memory_limit_bytes = number("memory.max");
    if out.memory_limit_bytes == 0 {
        return;
    }
    out.memory_charged_bytes = number("memory.current");
    if let Ok(events) = std::fs::read_to_string(format!("{dir}/memory.events")) {
        for line in events.lines() {
            match line.split_once(' ') {
                Some(("max", n)) => out.memory_ceiling_hits = n.trim().parse().unwrap_or(0),
                Some(("oom_kill", n)) => out.memory_oom_kills = n.trim().parse().unwrap_or(0),
                _ => {}
            }
        }
    }
}

#[cfg(target_os = "linux")]
fn clock_ticks_per_second() -> i64 {
    // SAFETY: sysconf with a constant name has no preconditions.
    let hz = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
    if hz > 0 { hz } else { 100 }
}

#[cfg(not(target_os = "linux"))]
pub fn read() -> Option<ProcessStatus> {
    None
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    #[test]
    fn the_process_can_describe_itself() {
        let p = super::read().expect("linux exposes /proc/self");
        assert!(p.rss_kib > 0);
        assert!(p.vm_kib >= p.rss_kib);
        assert!(p.threads >= 1);
        assert!(p.open_fds >= 3);
    }
}

/// Hands the allocator's free heap back to the kernel. glibc keeps what a
/// burst freed inside its arenas (an Argon2 hash is 19 MiB per call; a
/// removed revision's compiled image is tens of MiB), so RSS would show the
/// peak long after the work ended and read like a leak. Called after those
/// two bursts only — never on a request path — and a no-op off glibc.
pub fn release_heap() {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    // SAFETY: malloc_trim(0) takes no pointers and is safe to call at any
    // time; it only returns free memory the allocator already owns.
    unsafe {
        libc::malloc_trim(0);
    }
}
