//! Cron: runtime-declared scheduled finite work (`GOAL.md` §18).
//!
//! The scheduler lives as long as the revision is active; every tick is a
//! fresh finite world. Overlap `skip` drops a tick while the previous
//! invocation is still running; `allow` runs them concurrently.

use std::str::FromStr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Weak};

use chrono::{DateTime, Utc};
use croner::Cron;
use serde_json::json;
use tokio_util::sync::CancellationToken;

use crate::definition::{OverlapPolicy, Trigger};
use crate::runtime::{Revision, Runtime};
use crate::world::Termination;

#[derive(Debug, thiserror::Error)]
#[error("cron `{name}`: invalid schedule {schedule:?}: {detail}")]
pub struct InvalidSchedule {
    pub name: String,
    pub schedule: String,
    pub detail: String,
}

/// Validates every cron schedule in a definition. Called at install so a
/// bad schedule fails before the revision can serve.
pub fn validate(revision: &Revision) -> Result<(), InvalidSchedule> {
    for workload in revision.definition.workloads() {
        if let Trigger::Cron { schedule, .. } = &workload.trigger {
            Cron::from_str(schedule).map_err(|e| InvalidSchedule {
                name: workload.name.clone(),
                schedule: schedule.clone(),
                detail: e.to_string(),
            })?;
        }
    }
    Ok(())
}

#[derive(Default)]
pub struct CronStats {
    pub ticks: AtomicU64,
    pub skipped: AtomicU64,
    pub failed: AtomicU64,
}

/// Starts one scheduler loop per cron workload. Returns the token that
/// stops them all; the caller cancels it when the revision drains.
pub fn start(
    runtime: Weak<Runtime>,
    revision: Arc<Revision>,
    stats: Arc<CronStats>,
) -> CancellationToken {
    let stop = CancellationToken::new();
    for workload in revision.definition.workloads() {
        let Trigger::Cron {
            schedule, overlap, ..
        } = &workload.trigger
        else {
            continue;
        };
        let Ok(cron) = Cron::from_str(schedule) else {
            continue; // validated at install
        };
        let stop = stop.clone();
        let runtime = runtime.clone();
        let revision = Arc::clone(&revision);
        let stats = Arc::clone(&stats);
        let id = workload.id.clone();
        let name = workload.name.clone();
        let overlap = *overlap;
        tokio::spawn(async move {
            let running = Arc::new(AtomicBool::new(false));
            loop {
                let now = Utc::now();
                let Ok(next) = cron.find_next_occurrence(&now, false) else {
                    tracing::warn!(cron = %name, "no next occurrence; scheduler stopping");
                    return;
                };
                let wait = (next - now).to_std().unwrap_or_default();
                tokio::select! {
                    _ = tokio::time::sleep(wait) => {}
                    _ = stop.cancelled() => return,
                }
                stats.ticks.fetch_add(1, Ordering::SeqCst);
                if overlap == OverlapPolicy::Skip && running.load(Ordering::SeqCst) {
                    stats.skipped.fetch_add(1, Ordering::SeqCst);
                    tracing::info!(cron = %name, scheduled_at = %next, "tick skipped: previous invocation still running");
                    continue;
                }
                let Some(runtime) = runtime.upgrade() else {
                    return;
                };
                let revision = Arc::clone(&revision);
                let running = Arc::clone(&running);
                let stats = Arc::clone(&stats);
                let id = id.clone();
                let name = name.clone();
                let cancel = stop.child_token();
                running.store(true, Ordering::SeqCst);
                tokio::spawn(async move {
                    let outcome = run_tick(&runtime, &revision, &id, next, cancel).await;
                    running.store(false, Ordering::SeqCst);
                    if let Err(e) = outcome {
                        stats.failed.fetch_add(1, Ordering::SeqCst);
                        tracing::warn!(cron = %name, scheduled_at = %next, error = %e, "cron invocation failed");
                    }
                });
            }
        });
    }
    stop
}

/// One invocation in a fresh finite world. Shared by the scheduler and by
/// `Runtime::run_cron` (deterministic test/CLI invocation).
pub async fn run_tick(
    runtime: &Runtime,
    revision: &Arc<Revision>,
    workload_id: &str,
    scheduled_at: DateTime<Utc>,
    cancel: CancellationToken,
) -> Result<crate::world::WorkResult, String> {
    let admission = runtime
        .admit_in_flight(revision, workload_id)
        .map_err(|e| e.to_string())?;
    let input = super::input(
        revision,
        "cron",
        json!({ "scheduledAt": scheduled_at.to_rfc3339() }),
    );
    let result = runtime
        .execute(admission, input, cancel)
        .await
        .map_err(|e| e.to_string())?;
    match (&result.termination, &result.outcome) {
        (Termination::Completed, Some(Ok(_))) => Ok(result),
        (Termination::Completed, Some(Err(e))) => Err(format!("{}: {}", e.name, e.message)),
        (Termination::Completed, None) => Err("no outcome".into()),
        (Termination::DeadlineExceeded, _) => Err("deadline exceeded".into()),
        (Termination::Cancelled { reason }, _) => Err(format!("cancelled: {reason}")),
        (Termination::Faulted { detail }, _) => Err(format!("faulted: {detail}")),
    }
}
