//! `--exit-with-parent` / `USAI_EXIT_WITH_PARENT`: a supervisor that cannot
//! clean up after itself — a test runner killed by a CI cancel, an editor
//! task — used to leave the runtime it started serving, holding its database
//! pool, until the machine was rebooted. Round 23 found four of them from a
//! previous session still listening on one developer box.
use std::io::Read as _;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn fixture() -> Option<PathBuf> {
    if std::process::Command::new("node")
        .arg("--version")
        .output()
        .map(|o| !o.status.success())
        .unwrap_or(true)
    {
        return None;
    }
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../usai-runtime/tests/fixtures/http-app")
        .canonicalize()
        .ok()?;
    root.join("node_modules/@sakaladev/usai")
        .exists()
        .then_some(root)
}

fn alive(pid: i32) -> bool {
    // SAFETY: signal 0 only checks for the process's existence.
    unsafe { libc::kill(pid, 0) == 0 }
}

/// Starts a serving runtime under a shell, kills the shell, and reports
/// whether the runtime was still alive `wait` later. The runtime is killed
/// either way, so neither outcome leaks a process.
fn outlives_its_parent(root: &str, flag: &[&str], wait: Duration) -> bool {
    let usai = env!("CARGO_BIN_EXE_usai");
    let flags = flag.join(" ");
    let shell = format!(
        "{usai} {flags} --root {root} run --port 0 --control 127.0.0.1:0 > /dev/null 2>&1 & \
         echo $! ; sleep 120"
    );
    let mut parent = Command::new("sh")
        .arg("-c")
        .arg(&shell)
        .env("UPSTREAM_URL", "http://127.0.0.1:9/")
        .stdout(Stdio::piped())
        .spawn()
        .expect("sh runs");
    let mut pid = String::new();
    {
        let mut out = parent.stdout.take().expect("piped");
        let mut buf = [0u8; 32];
        let n = out.read(&mut buf).unwrap_or(0);
        pid.push_str(String::from_utf8_lossy(&buf[..n]).trim());
    }
    let child: i32 = pid.parse().expect("the shell printed the runtime's pid");
    // Let it get past start-up, so this is about the flag and not a race
    // with the build.
    std::thread::sleep(Duration::from_secs(3));
    assert!(
        alive(child),
        "the runtime did not stay up long enough to test"
    );
    let _ = parent.kill();
    let _ = parent.wait();
    let started = Instant::now();
    while alive(child) && started.elapsed() < wait {
        std::thread::sleep(Duration::from_millis(100));
    }
    let survived = alive(child);
    // SAFETY: a pid this test started; SIGKILL, because the point of the
    // test is the case where nothing asked it to stop.
    unsafe {
        libc::kill(child, libc::SIGKILL);
    }
    survived
}

#[test]
fn a_runtime_exits_when_the_process_that_started_it_dies() {
    let Some(root) = fixture() else { return };
    let root = root.to_str().expect("utf-8 path").to_owned();
    assert!(
        !outlives_its_parent(&root, &["--exit-with-parent"], Duration::from_secs(20)),
        "the runtime outlived the process that started it"
    );
    // The control: without the flag it stays, which is what makes the
    // assertion above a result rather than a coincidence of timing.
    assert!(
        outlives_its_parent(&root, &[], Duration::from_secs(5)),
        "the runtime went away without being asked to; this test proves nothing"
    );
}
