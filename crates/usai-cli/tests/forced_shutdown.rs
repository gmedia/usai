//! D13: graceful shutdown drains; a second interrupt forces exit and reports
//! what was still alive.
mod support;

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const STUCK_APP: &str = r#"
import { defineApp, http, service } from "@sakaladev/usai";
// A service that ignores its stop signal and keeps arming timers: the
// drain can only end by its timeout, which is what the forced path is for.
export const stuck = service("stuck", async (ctx) => { for (;;) { await ctx.sleep(60000); } });
export const hello = http.get("/hello/:name", {}, async (ctx) => ({ hello: ctx.params.name }));
export default defineApp({ name: "stuck", workloads: [hello, stuck] });
"#;

#[test]
fn second_interrupt_forces_exit_and_reports_live_work() {
    if !support::node_available() {
        return;
    }
    let Some(dir) = support::project("forced", STUCK_APP) else {
        eprintln!("skipping: run pnpm install first");
        return;
    };
    let port = support::free_port();
    let mut child = Command::new(env!("CARGO_BIN_EXE_usai"))
        .args([
            "--root",
            dir.to_str().unwrap(),
            "run",
            "--port",
            &port.to_string(),
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let log = support::capture(child.stderr.take().unwrap());
    let deadline = Instant::now() + Duration::from_secs(120);
    while support::get(port, "/hello/x").map(|(s, _)| s) != Some(200) {
        assert!(
            Instant::now() < deadline,
            "server did not come up:\n{}",
            log.lock().unwrap()
        );
        std::thread::sleep(Duration::from_millis(200));
    }
    let pid = child.id() as i32;
    unsafe { libc::kill(pid, libc::SIGINT) };
    assert!(
        support::wait_for_log(&log, "draining in-flight work", Duration::from_secs(10)),
        "{}",
        log.lock().unwrap()
    );
    // The stuck service keeps the drain open; a second interrupt ends it now.
    std::thread::sleep(Duration::from_millis(300));
    assert!(
        child.try_wait().unwrap().is_none(),
        "drained too early:\n{}",
        log.lock().unwrap()
    );
    unsafe { libc::kill(pid, libc::SIGINT) };
    let t = Instant::now();
    let status = loop {
        if let Some(s) = child.try_wait().unwrap() {
            break s;
        }
        assert!(
            t.elapsed() < Duration::from_secs(5),
            "forced shutdown did not exit:\n{}",
            log.lock().unwrap()
        );
        std::thread::sleep(Duration::from_millis(50));
    };
    assert_eq!(status.code(), Some(130));
    let log = log.lock().unwrap();
    // Log lines carry ANSI colour in the capture; compare without it.
    let plain: String = {
        let mut out = String::new();
        let mut chars = log.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '\x1b' {
                for c in chars.by_ref() {
                    if c == 'm' {
                        break;
                    }
                }
            } else {
                out.push(c);
            }
        }
        out
    };
    assert!(
        plain.contains("forced shutdown") && plain.contains("live_worlds=1"),
        "{log}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

const PLAIN_APP: &str = r#"
import { defineApp, http } from "@sakaladev/usai";
export const hello = http.get("/hello/:name", {}, async (ctx) => ({ hello: ctx.params.name }));
export default defineApp({ name: "plain", workloads: [hello] });
"#;

/// The signal handlers are installed before the listener answers anything, so
/// a signal that arrives with the very first request still drains. They used
/// to be installed when the shutdown `select!` was first polled — after the
/// banner, after the first request could be served — and a SIGTERM landing in
/// that window took the process's default action and killed it. An
/// orchestrator that changes its mind about a deploy lands exactly there.
#[test]
fn a_signal_arriving_with_the_first_request_still_drains() {
    if !support::node_available() {
        return;
    }
    let Some(dir) = support::project("early-signal", PLAIN_APP) else {
        eprintln!("skipping: run pnpm install first");
        return;
    };
    let port = support::free_port();
    let mut child = Command::new(env!("CARGO_BIN_EXE_usai"))
        .args([
            "--root",
            dir.to_str().unwrap(),
            "run",
            "--port",
            &port.to_string(),
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let log = support::capture(child.stderr.take().unwrap());
    let deadline = Instant::now() + Duration::from_secs(120);
    while support::get(port, "/hello/x").map(|(s, _)| s) != Some(200) {
        assert!(
            Instant::now() < deadline,
            "server did not come up:\n{}",
            log.lock().unwrap()
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    // No pause: the first response and the signal are as close together as
    // the test can make them.
    unsafe { libc::kill(child.id() as i32, libc::SIGTERM) };
    assert!(
        support::wait_for_log(&log, "draining in-flight work", Duration::from_secs(20)),
        "the signal was not handled as a drain:\n{}",
        log.lock().unwrap()
    );
    let t = Instant::now();
    let status = loop {
        if let Some(s) = child.try_wait().unwrap() {
            break s;
        }
        assert!(
            t.elapsed() < Duration::from_secs(30),
            "it did not exit:\n{}",
            log.lock().unwrap()
        );
        std::thread::sleep(Duration::from_millis(50));
    };
    assert_eq!(
        status.code(),
        Some(0),
        "a drained exit is 0:\n{}",
        log.lock().unwrap()
    );
}
