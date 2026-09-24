//! `USAI_ACTIVATION_RETRY`: a dependency that is not there yet.
//!
//! **60 seconds by default** (ADR-0022). Activation still fails rather than
//! the first request, and the process still exits with the dependency's own
//! error when the budget ends — what changed is that it does not exit
//! *immediately*, because on an orchestrator that is `CrashLoopBackOff` with
//! a backoff that outlives the outage which caused it: a pod restarted for
//! an unrelated reason during a database blip stayed down long after the
//! database was healthy.
//!
//! `USAI_ACTIVATION_RETRY=0` restores the immediate exit. A missing or
//! malformed **variable** is never retried either way — no amount of waiting
//! fixes a configuration error.
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

/// `0` is the old behaviour and stays available: exit at once, no retry.
/// A VM or a local run wants this, and so does anyone who would rather see
/// the error than wait a minute for it.
#[test]
fn a_zero_budget_ends_the_process_at_once() {
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
        .env("USAI_ACTIVATION_RETRY", "0")
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
        "a zero budget must not retry: {text}"
    );
    assert!(
        started.elapsed() < Duration::from_secs(20),
        "it should give up at once, took {:?}",
        started.elapsed()
    );
}

/// And the **default** is a budget, not an immediate exit. This is the
/// behaviour every deployment that sets nothing now gets, so it is the one
/// that has to be stated: the process stays up, says so, and keeps trying.
#[test]
fn the_default_is_a_budget_so_an_orchestrator_does_not_crash_loop() {
    if !support::node_available() {
        return;
    }
    let Some(dir) = support::project("retry-default", APP) else {
        return;
    };
    if !build(&dir) {
        return;
    }
    let started = Instant::now();
    let mut child = Command::new(env!("CARGO_BIN_EXE_usai"))
        .args(["--root", dir.to_str().unwrap(), "run", "--port", "3943"])
        .env("DATABASE_URL", DEAD)
        .env_remove("USAI_ACTIVATION_RETRY")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    // Well inside the 60 s default: it must still be alive and retrying.
    std::thread::sleep(Duration::from_secs(12));
    let alive = child.try_wait().unwrap().is_none();
    let _ = child.kill();
    let out = child.wait_with_output().unwrap();
    let text =
        String::from_utf8_lossy(&out.stderr).to_string() + &String::from_utf8_lossy(&out.stdout);
    assert!(
        alive,
        "the default exited on a dependency after {:?}; an orchestrator reads that as a crash loop:\n{text}",
        started.elapsed()
    );
    assert!(
        text.contains("retrying"),
        "it must say it is staying up and why: {text}"
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
