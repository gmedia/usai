//! `USAI_ACTIVATION_RETRY`: a dependency that is not there yet.
//!
//! Without it, an unreachable resource ends the process before any listener
//! binds — right for a missing variable, right on a VM, and on an
//! orchestrator a restart loop whose backoff outlives the outage that caused
//! it (`docs/OPEN-QUESTIONS.md` → Q20). With it, the process stays up and
//! keeps trying for a bounded time, so a startup probe covers the blip.
mod support;

use std::process::Command;
use std::time::{Duration, Instant};

/// An application that needs a database nothing is listening for.
const APP: &str = r#"
import { defineApp, http, postgres } from "@sakaladev/usai";
import { z } from "zod";

const db = postgres("db");

export const notes = http.get(
  "/notes",
  { response: z.array(z.object({ id: z.number().int() })), resources: [db] },
  async (ctx) => ctx.resources.db.query<{ id: number }>("select 1 as id"),
);

export default defineApp({ name: "retry", resources: [db], workloads: [notes] });
"#;

fn build(dir: &std::path::Path) -> bool {
    Command::new(env!("CARGO_BIN_EXE_usai"))
        .args(["--root", dir.to_str().unwrap(), "build", "--no-typecheck"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// A port on loopback that nothing serves: connecting is refused, which is
/// the fast shape of "the database is not there".
const DEAD: &str = "postgres://nobody@127.0.0.1:59941/nothing?connect_timeout=1";

#[test]
fn a_dependency_that_is_not_there_ends_the_process_by_default() {
    if !support::node_available() {
        return;
    }
    let Some(dir) = support::project("retry-off", APP) else {
        return;
    };
    if !build(&dir) {
        return;
    }
    let started = Instant::now();
    let out = Command::new(env!("CARGO_BIN_EXE_usai"))
        .args(["--root", dir.to_str().unwrap(), "run", "--port", "3941"])
        .env("DATABASE_URL", DEAD)
        .env_remove("USAI_ACTIVATION_RETRY")
        .output()
        .unwrap();
    let text =
        String::from_utf8_lossy(&out.stderr).to_string() + &String::from_utf8_lossy(&out.stdout);
    assert!(
        text.contains("failed to start"),
        "the refusal must name the resource: {text}"
    );
    assert!(
        !text.contains("retrying"),
        "nothing should retry without the variable: {text}"
    );
    assert!(
        started.elapsed() < Duration::from_secs(20),
        "it should give up at once, took {:?}",
        started.elapsed()
    );
}

#[test]
fn with_a_retry_budget_it_stays_up_and_keeps_trying_until_the_budget_ends() {
    if !support::node_available() {
        return;
    }
    let Some(dir) = support::project("retry-on", APP) else {
        return;
    };
    if !build(&dir) {
        return;
    }
    let started = Instant::now();
    let out = Command::new(env!("CARGO_BIN_EXE_usai"))
        .args(["--root", dir.to_str().unwrap(), "run", "--port", "3942"])
        .env("DATABASE_URL", DEAD)
        .env("USAI_ACTIVATION_RETRY", "8")
        .output()
        .unwrap();
    let elapsed = started.elapsed();
    let text =
        String::from_utf8_lossy(&out.stderr).to_string() + &String::from_utf8_lossy(&out.stdout);
    assert!(
        text.matches("retrying").count() >= 2,
        "it should have retried more than once in eight seconds: {text}"
    );
    assert!(
        elapsed >= Duration::from_secs(7),
        "it gave up before its budget ({elapsed:?})"
    );
    assert!(
        elapsed < Duration::from_secs(30),
        "it outlived its budget ({elapsed:?})"
    );
    // The budget ends the way it always did: the dependency's own error.
    assert!(
        text.contains("failed to start"),
        "the last word is still the dependency: {text}"
    );
    // And the revisions each attempt installed are not left behind — the
    // bound is eight, and a minute of retries would otherwise trip it.
    assert!(
        !text.contains("revisions are held"),
        "a failed attempt left its revision behind: {text}"
    );
}
