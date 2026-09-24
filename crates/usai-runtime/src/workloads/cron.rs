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
use crate::resource::ResourceManager;
use crate::runtime::{Revision, Runtime};
use crate::world::Termination;

/// The ledger an `exclusive` schedule claims its ticks in: one row per
/// (schedule, scheduled time), inserted by whichever replica gets there
/// first. Rows older than a week are pruned by the claimants.
pub const TICKS_SCHEMA: &str = "CREATE TABLE IF NOT EXISTS usai_cron_ticks (
  name text NOT NULL,
  scheduled_at timestamptz NOT NULL,
  claimed_by text NOT NULL,
  claimed_at timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (name, scheduled_at)
)";

fn hostname() -> Option<String> {
    let mut buf = [0u8; 256];
    // SAFETY: gethostname writes at most `buf.len()` bytes into a buffer we own.
    let rc = unsafe { libc::gethostname(buf.as_mut_ptr().cast(), buf.len()) };
    if rc != 0 {
        return None;
    }
    let end = buf.iter().position(|b| *b == 0).unwrap_or(buf.len());
    String::from_utf8(buf[..end].to_vec()).ok()
}

/// Claims one tick for this instance: `true` when this call inserted the
/// row, `false` when another instance already had. A database error is
/// reported as `Err` and the caller decides (the scheduler runs the tick —
/// a flaky database must not silence a schedule on every replica at once).
pub async fn claim_tick(
    manager: &dyn ResourceManager,
    name: &str,
    scheduled_at: DateTime<Utc>,
    claimed_by: &str,
) -> Result<bool, String> {
    let claimed = super::queue::sql(
        manager,
        "execute",
        "INSERT INTO usai_cron_ticks (name, scheduled_at, claimed_by) VALUES ($1, $2, $3) ON CONFLICT DO NOTHING",
        vec![json!(name), json!(scheduled_at.to_rfc3339()), json!(claimed_by)],
    )
    .await;
    let claimed = match claimed {
        Ok(n) => n.as_u64().unwrap_or(0) == 1,
        // The table may not exist yet on a fresh database: prepare it once
        // and claim again (CREATE TABLE IF NOT EXISTS races are tolerated
        // the way the queue's are).
        Err(e) if e.contains("usai_cron_ticks") || e.contains("42P01") => {
            let _ = super::queue::sql(manager, "execute", TICKS_SCHEMA, vec![]).await;
            super::queue::sql(
                manager,
                "execute",
                "INSERT INTO usai_cron_ticks (name, scheduled_at, claimed_by) VALUES ($1, $2, $3) ON CONFLICT DO NOTHING",
                vec![json!(name), json!(scheduled_at.to_rfc3339()), json!(claimed_by)],
            )
            .await?
            .as_u64()
            .unwrap_or(0)
                == 1
        }
        Err(e) => return Err(e),
    };
    if claimed {
        let _ = super::queue::sql(
            manager,
            "execute",
            "DELETE FROM usai_cron_ticks WHERE name = $1 AND scheduled_at < now() - interval '7 days'",
            vec![json!(name)],
        )
        .await;
    }
    Ok(claimed)
}

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
        if let Trigger::Cron {
            schedule,
            exclusive,
            database,
            ..
        } = &workload.trigger
        {
            Cron::from_str(schedule).map_err(|e| InvalidSchedule {
                name: workload.name.clone(),
                schedule: schedule.clone(),
                detail: e.to_string(),
            })?;
            // An exclusive schedule claims its ticks through PostgreSQL:
            // refuse at install when there is nothing to claim through.
            if *exclusive {
                let resources = revision.definition.resources();
                let found = match database {
                    Some(name) => resources
                        .iter()
                        .any(|r| r.kind == "postgres" && r.name == *name),
                    None => resources.iter().any(|r| r.kind == "postgres"),
                };
                if !found {
                    return Err(InvalidSchedule {
                        name: workload.name.clone(),
                        schedule: schedule.clone(),
                        detail: match database {
                            Some(name) => format!("exclusive: true names database {name:?}, which is not a postgres resource of this application"),
                            None => "exclusive: true needs a postgres resource to claim ticks through (declare one, or name it with exclusive: { database })".to_owned(),
                        },
                    });
                }
            }
        }
    }
    Ok(())
}

#[derive(Default)]
pub struct CronStats {
    pub ticks: AtomicU64,
    pub skipped: AtomicU64,
    pub failed: AtomicU64,
    /// Ticks of an `exclusive` schedule another instance claimed first.
    pub taken: AtomicU64,
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
            schedule,
            overlap,
            exclusive,
            database,
            ..
        } = &workload.trigger
        else {
            continue;
        };
        let Ok(cron) = Cron::from_str(schedule) else {
            continue; // validated at install
        };
        let claim_through = if *exclusive {
            match super::queue::database_for(&revision, database.as_deref()) {
                Some(m) => Some(m),
                None => {
                    tracing::error!(cron = %workload.name, "exclusive schedule without a postgres resource to claim ticks through; scheduler not started");
                    continue;
                }
            }
        } else {
            None
        };
        // Who claimed: revision, schedule, and the instance (host:pid), so the
        // ledger tells replicas apart.
        let claimant = format!(
            "{}:{}@{}:{}",
            revision.id,
            workload.name,
            hostname().unwrap_or_else(|| "?".into()),
            std::process::id()
        );
        let stop = stop.clone();
        let runtime = runtime.clone();
        let revision = Arc::clone(&revision);
        let stats = Arc::clone(&stats);
        let id = workload.id.clone();
        let name = workload.name.clone();
        let overlap = *overlap;
        tokio::spawn(async move {
            let running = Arc::new(AtomicBool::new(false));
            let claim_through = claim_through;
            let claimant = claimant;
            // The last scheduled time this scheduler fired. A tick is for a
            // *scheduled time*, not for a moment the timer woke up, and
            // firing one twice is a double send for anything that is not
            // idempotent.
            let mut last_fired: Option<DateTime<Utc>> = None;
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
                // A sleep computed from the wall clock and served by the
                // monotonic one can end early — measured at ~0.4 s before
                // the boundary on a `*/1 * * * *` schedule, which then fired
                // at 59.6 s *and* at 00.0 s: the loop recomputed the same
                // occurrence and slept the remainder. Wait out the
                // difference instead of treating an early wake as the tick.
                loop {
                    let short_by = next - Utc::now();
                    match short_by.to_std() {
                        Ok(remaining) if !remaining.is_zero() => {
                            tokio::select! {
                                _ = tokio::time::sleep(remaining) => {}
                                _ = stop.cancelled() => return,
                            }
                        }
                        _ => break,
                    }
                }
                // And never the same scheduled time twice, whatever the
                // clock did in between (a step backwards is a real thing on
                // a virtualised host).
                if last_fired == Some(next) {
                    continue;
                }
                last_fired = Some(next);
                stats.ticks.fetch_add(1, Ordering::SeqCst);
                if overlap == OverlapPolicy::Skip && running.load(Ordering::SeqCst) {
                    stats.skipped.fetch_add(1, Ordering::SeqCst);
                    tracing::info!(cron = %name, scheduled_at = %next, "tick skipped: previous invocation still running");
                    continue;
                }
                if let Some(manager) = &claim_through {
                    match claim_tick(manager.as_ref(), &name, next, &claimant).await {
                        Ok(true) => {}
                        Ok(false) => {
                            stats.taken.fetch_add(1, Ordering::SeqCst);
                            tracing::debug!(cron = %name, scheduled_at = %next, "tick claimed by another instance");
                            continue;
                        }
                        Err(e) => {
                            tracing::warn!(cron = %name, scheduled_at = %next, error = %e, "could not claim the tick; running it here (the database is down for every replica alike)");
                        }
                    }
                }
                let Some(runtime) = runtime.upgrade() else {
                    return;
                };
                let revision = Arc::clone(&revision);
                let running = Arc::clone(&running);
                let stats = Arc::clone(&stats);
                let id = id.clone();
                let name = name.clone();
                // **Not** a child of the scheduler's stop. That token means
                // "stop scheduling", and it is cancelled at the start of a
                // drain — so a tick already running was cancelled outright
                // 1.5 ms after SIGTERM, mid-batch, instead of being asked to
                // stop and given the drain window. The graceful stop it does
                // get is the revision's (`run_tick` below); the hard cancel
                // is the runtime's shutdown token, which `execute_opts`
                // merges in for every world.
                let cancel = CancellationToken::new();
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
        .execute_with_stop(admission, input, cancel, Some(revision.connections_stop()))
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
