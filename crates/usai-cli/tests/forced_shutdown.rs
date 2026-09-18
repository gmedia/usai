//! D13: graceful shutdown drains; a second interrupt forces exit and reports
//! what was still alive.
mod support;

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const STUCK_APP: &str = r#"
import { defineApp, http, service } from "usai";
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
    assert!(log.contains("forced shutdown with 1 live worlds"), "{log}");
    let _ = std::fs::remove_dir_all(&dir);
}
