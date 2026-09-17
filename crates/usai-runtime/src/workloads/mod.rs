//! Non-HTTP workload triggers: tasks (D4), cron and commands (D5).
//!
//! Each trigger composes an input for the SDK dispatcher and runs one finite
//! world through the same admission/execution path HTTP uses. Nothing here
//! keeps a permanent mutable application process alive.

pub mod cron;
pub mod tasks;

use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::runtime::Revision;

/// The `{kind, env, …}` envelope the SDK's `invoke` expects
/// (`docs/GUEST-ABI.md`).
pub fn input(revision: &Revision, kind: &str, fields: Value) -> Value {
    let env: BTreeMap<String, String> = (*revision.env()).clone();
    let mut envelope = json!({ "kind": kind, "env": env });
    if let (Value::Object(target), Value::Object(extra)) = (&mut envelope, fields) {
        target.extend(extra);
    }
    envelope
}
