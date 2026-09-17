//! Ownership bookkeeping for worlds and the external operations they start.
//!
//! The ledger is the production form of the research repo's indexed
//! physical-operation ledger (EXP-012B §4.1): indexed by operation id and by
//! owning world, and bounded by *live* work. A released record is gone; there
//! is no process-lifetime history here. Anything that wants history (tracing,
//! metrics) subscribes to events instead of reading the ledger.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use serde::Serialize;

/// Identity of one execution world. Unique for the runtime's lifetime, so a
/// completion addressed to a dead world can never match a live one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
pub struct WorldId(pub u64);

impl std::fmt::Display for WorldId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "world#{}", self.0)
    }
}

/// Identity of one external operation. Unique for the runtime's lifetime.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
pub struct OpId(pub u64);

impl std::fmt::Display for OpId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "op#{}", self.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum OpState {
    /// Started; the owner task is running it.
    Running,
    /// The owning world gave up interest (cancel, deadline, or death). The
    /// physical work may still be running; its owner will still reach a
    /// terminal state and release the record.
    LogicallyCancelled,
}

#[derive(Clone, Debug, Serialize)]
pub struct OpRecord {
    pub op: OpId,
    pub world: WorldId,
    pub kind: String,
    pub state: OpState,
}

#[derive(Default)]
struct Inner {
    by_op: HashMap<OpId, OpRecord>,
    by_world: HashMap<WorldId, Vec<OpId>>,
}

/// Runtime-wide gauges. Every value here must return to its baseline when all
/// work has settled; tests assert exactly that.
#[derive(Debug, Default)]
pub struct Gauges {
    pub live_worlds: AtomicU64,
    pub live_ops: AtomicU64,
    pub completions_delivered: AtomicU64,
    pub completions_dropped_late: AtomicU64,
    pub completions_rejected_stale: AtomicU64,
    pub detached_work_detected: AtomicU64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct GaugeSnapshot {
    pub live_worlds: u64,
    pub live_ops: u64,
    pub completions_delivered: u64,
    pub completions_dropped_late: u64,
    pub completions_rejected_stale: u64,
    pub detached_work_detected: u64,
}

impl Gauges {
    pub fn snapshot(&self) -> GaugeSnapshot {
        GaugeSnapshot {
            live_worlds: self.live_worlds.load(Ordering::SeqCst),
            live_ops: self.live_ops.load(Ordering::SeqCst),
            completions_delivered: self.completions_delivered.load(Ordering::SeqCst),
            completions_dropped_late: self.completions_dropped_late.load(Ordering::SeqCst),
            completions_rejected_stale: self.completions_rejected_stale.load(Ordering::SeqCst),
            detached_work_detected: self.detached_work_detected.load(Ordering::SeqCst),
        }
    }
}

pub(crate) fn inc(counter: &AtomicU64) {
    counter.fetch_add(1, Ordering::SeqCst);
}

pub(crate) fn dec(counter: &AtomicU64) {
    let previous = counter.fetch_sub(1, Ordering::SeqCst);
    debug_assert!(previous > 0, "gauge underflow");
}

/// Live-ownership ledger plus id allocation. One per runtime.
#[derive(Default)]
pub struct Ledger {
    next_world: AtomicU64,
    next_op: AtomicU64,
    inner: Mutex<Inner>,
    pub gauges: Arc<Gauges>,
}

impl Ledger {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn next_world_id(&self) -> WorldId {
        WorldId(self.next_world.fetch_add(1, Ordering::SeqCst) + 1)
    }

    /// Registers a new operation owned by `world`. The record lives until
    /// `release`, which the operation's owner calls on every path.
    pub fn start(&self, world: WorldId, kind: &str) -> OpId {
        let op = OpId(self.next_op.fetch_add(1, Ordering::SeqCst) + 1);
        let mut inner = self.inner.lock().expect("ledger poisoned");
        inner.by_op.insert(
            op,
            OpRecord {
                op,
                world,
                kind: kind.to_owned(),
                state: OpState::Running,
            },
        );
        inner.by_world.entry(world).or_default().push(op);
        inc(&self.gauges.live_ops);
        op
    }

    /// Marks every operation the world owns as logically cancelled and
    /// returns them. Physical records stay until their owners release them.
    pub fn cancel_world(&self, world: WorldId) -> Vec<OpId> {
        let mut inner = self.inner.lock().expect("ledger poisoned");
        let ops = inner.by_world.get(&world).cloned().unwrap_or_default();
        for op in &ops {
            if let Some(record) = inner.by_op.get_mut(op) {
                record.state = OpState::LogicallyCancelled;
            }
        }
        ops
    }

    /// Guest-initiated cancel of one operation (e.g. `clearTimeout`). The
    /// record stays until its owner releases it.
    pub fn cancel_op(&self, op: OpId, world: WorldId) -> bool {
        let mut inner = self.inner.lock().expect("ledger poisoned");
        match inner.by_op.get_mut(&op) {
            Some(record) if record.world == world => {
                record.state = OpState::LogicallyCancelled;
                true
            }
            _ => false,
        }
    }

    /// Whether a completion for `op` may still be delivered to `world`.
    pub fn is_deliverable(&self, world: WorldId, op: OpId) -> bool {
        let inner = self.inner.lock().expect("ledger poisoned");
        matches!(
            inner.by_op.get(&op),
            Some(record) if record.world == world && record.state == OpState::Running
        )
    }

    /// Releases the record. Called exactly once by the operation owner when
    /// the physical work reached a terminal state, on every path.
    pub fn release(&self, op: OpId) -> Option<OpRecord> {
        let mut inner = self.inner.lock().expect("ledger poisoned");
        let record = inner.by_op.remove(&op)?;
        if let Some(ops) = inner.by_world.get_mut(&record.world) {
            ops.retain(|o| *o != op);
            if ops.is_empty() {
                inner.by_world.remove(&record.world);
            }
        }
        dec(&self.gauges.live_ops);
        Some(record)
    }

    pub fn outstanding_for(&self, world: WorldId) -> Vec<OpRecord> {
        let inner = self.inner.lock().expect("ledger poisoned");
        inner
            .by_world
            .get(&world)
            .map(|ops| {
                ops.iter()
                    .filter_map(|op| inner.by_op.get(op).cloned())
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn live_records(&self) -> usize {
        self.inner.lock().expect("ledger poisoned").by_op.len()
    }
}

/// RAII owner of one ledger record. Dropping it releases the record, so an
/// operation task that panics or is aborted still returns ownership to
/// baseline.
pub struct OpGuard {
    ledger: Arc<Ledger>,
    op: OpId,
    released: bool,
}

impl OpGuard {
    pub fn new(ledger: Arc<Ledger>, op: OpId) -> Self {
        Self {
            ledger,
            op,
            released: false,
        }
    }

    pub fn op(&self) -> OpId {
        self.op
    }

    pub fn release(mut self) -> Option<OpRecord> {
        self.released = true;
        self.ledger.release(self.op)
    }
}

impl Drop for OpGuard {
    fn drop(&mut self) {
        if !self.released {
            self.ledger.release(self.op);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ledger_is_bounded_by_live_work() {
        let ledger = Ledger::new();
        let w = ledger.next_world_id();
        let a = ledger.start(w, "x");
        let b = ledger.start(w, "y");
        assert_eq!(ledger.live_records(), 2);
        assert!(ledger.is_deliverable(w, a));
        ledger.release(a);
        ledger.release(b);
        assert_eq!(ledger.live_records(), 0);
        assert_eq!(ledger.gauges.snapshot().live_ops, 0);
        assert!(!ledger.is_deliverable(w, a));
    }

    #[test]
    fn logical_cancel_keeps_the_physical_record() {
        let ledger = Ledger::new();
        let w = ledger.next_world_id();
        let a = ledger.start(w, "x");
        assert_eq!(ledger.cancel_world(w), vec![a]);
        assert!(!ledger.is_deliverable(w, a));
        assert_eq!(ledger.live_records(), 1, "cancel is not release");
        ledger.release(a);
        assert_eq!(ledger.live_records(), 0);
    }

    #[test]
    fn a_completion_for_another_world_is_not_deliverable() {
        let ledger = Ledger::new();
        let w1 = ledger.next_world_id();
        let w2 = ledger.next_world_id();
        let a = ledger.start(w1, "x");
        assert!(!ledger.is_deliverable(w2, a));
    }

    #[test]
    fn guard_releases_on_drop() {
        let ledger = Ledger::new();
        let w = ledger.next_world_id();
        let op = ledger.start(w, "x");
        {
            let _guard = OpGuard::new(ledger.clone(), op);
        }
        assert_eq!(ledger.live_records(), 0);
    }
}
