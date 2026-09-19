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
    Some(out)
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
