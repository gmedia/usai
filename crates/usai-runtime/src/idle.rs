//! "Is any world alive?" — the one question the runtime's two periodic
//! threads (the watchdog, `world.rs`; the epoch ticker, `engine/wasm.rs`)
//! ask before waking. With no world live there is no deadline to enforce
//! and no guest to interrupt, so both park here instead of waking 300
//! times a second in an idle process; a fleet of mostly-idle applications
//! then costs what it holds, not what it polls.

use std::sync::{Condvar, Mutex};

struct Activity {
    live: Mutex<u64>,
    changed: Condvar,
}

static ACTIVITY: Activity = Activity {
    live: Mutex::new(0),
    changed: Condvar::new(),
};

/// A world (or anything that needs the tickers) started.
pub fn enter() {
    let mut live = ACTIVITY.live.lock().expect("activity poisoned");
    *live += 1;
    if *live == 1 {
        ACTIVITY.changed.notify_all();
    }
}

/// The counterpart of `enter`.
pub fn leave() {
    let mut live = ACTIVITY.live.lock().expect("activity poisoned");
    *live = live.saturating_sub(1);
}

/// Blocks the calling thread until at least one world is live. Returns at
/// once when one is.
pub fn wait_until_active() {
    let mut live = ACTIVITY.live.lock().expect("activity poisoned");
    while *live == 0 {
        live = ACTIVITY.changed.wait(live).expect("activity poisoned");
    }
}

/// Live worlds as the tickers see them (tests, status).
pub fn live() -> u64 {
    *ACTIVITY.live.lock().expect("activity poisoned")
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_parked_waiter_wakes_on_enter() {
        // Other tests in this process create and drop worlds concurrently,
        // so the count itself is not asserted — only that a parked waiter
        // returns once something is live, and that the pair balances.
        let waiter = std::thread::spawn(|| {
            super::wait_until_active();
        });
        super::enter();
        waiter.join().unwrap();
        assert!(super::live() >= 1);
        super::leave();
    }
}
