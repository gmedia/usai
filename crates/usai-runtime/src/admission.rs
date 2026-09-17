//! Hierarchical admission budgets (ADR-0012).
//!
//! A budget is a bounded counter that hands out permits. Admission consults
//! runtime -> application -> workload budgets before a world exists, and
//! resource managers consult their own before a lease exists. There is no
//! single global knob.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use tokio::sync::{OwnedSemaphorePermit, Semaphore, TryAcquireError};

#[derive(Debug)]
pub struct Budget {
    name: String,
    max: u32,
    semaphore: Arc<Semaphore>,
    rejected: AtomicU64,
}

/// Holding a permit means the budget has counted you. Dropping it returns the
/// capacity.
pub struct Permit {
    _inner: OwnedSemaphorePermit,
}

#[derive(Debug, thiserror::Error)]
#[error("{budget} budget exhausted ({max} in use)")]
pub struct BudgetExhausted {
    pub budget: String,
    pub max: u32,
}

impl Budget {
    pub fn new(name: impl Into<String>, max: u32) -> Arc<Self> {
        Arc::new(Self {
            name: name.into(),
            max,
            semaphore: Arc::new(Semaphore::new(max as usize)),
            rejected: AtomicU64::new(0),
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn max(&self) -> u32 {
        self.max
    }

    pub fn in_use(&self) -> u32 {
        self.max - self.semaphore.available_permits() as u32
    }

    pub fn rejected(&self) -> u64 {
        self.rejected.load(Ordering::Relaxed)
    }

    /// Non-blocking admission. Work that cannot be admitted is refused at the
    /// boundary; it never waits inside a world.
    pub fn try_acquire(&self) -> Result<Permit, BudgetExhausted> {
        match self.semaphore.clone().try_acquire_owned() {
            Ok(inner) => Ok(Permit { _inner: inner }),
            Err(TryAcquireError::NoPermits) | Err(TryAcquireError::Closed) => {
                self.rejected.fetch_add(1, Ordering::Relaxed);
                Err(BudgetExhausted {
                    budget: self.name.clone(),
                    max: self.max,
                })
            }
        }
    }

    /// Bounded wait, for callers that have declared a queueing policy.
    pub async fn acquire(&self) -> Permit {
        let inner = self
            .semaphore
            .clone()
            .acquire_owned()
            .await
            .expect("budget semaphore is never closed");
        Permit { _inner: inner }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permits_are_returned_on_drop() {
        let b = Budget::new("t", 1);
        let p = b.try_acquire().unwrap();
        assert!(b.try_acquire().is_err());
        assert_eq!(b.rejected(), 1);
        drop(p);
        assert!(b.try_acquire().is_ok());
    }
}
