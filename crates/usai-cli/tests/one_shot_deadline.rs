//! A one-shot that runs past its deadline used to report
//! `work ended without a result: DeadlineExceeded` — no workload, no
//! elapsed, no `cpu_us`, and no hint that the bound is the runtime's 30 s
//! default for a workload that declares none. For a batched backfill run
//! with `usai app`, that is the difference between "declare a timeout" and
//! twenty minutes of thinking the runtime faulted (round 24).
use std::path::PathBuf;
use std::process::Command;

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

#[test]
fn a_one_shot_that_hits_its_deadline_says_which_and_how_long() {
    let Some(root) = fixture() else { return };
    let out = Command::new(env!("CARGO_BIN_EXE_usai"))
        .args([
            "--root",
            root.to_str().expect("utf-8 path"),
            "task",
            "run",
            "over-deadline",
        ])
        .env("UPSTREAM_URL", "http://127.0.0.1:9/")
        .output()
        .expect("usai runs");
    assert!(!out.status.success(), "the deadline must fail the command");
    let text = String::from_utf8_lossy(&out.stderr);
    assert!(
        text.contains("task:over-deadline"),
        "it must name the workload: {text}"
    );
    assert!(
        text.contains("hit its deadline after"),
        "it must say how long it got: {text}"
    );
    assert!(
        text.contains("timeout:"),
        "it must say the bound is declarable: {text}"
    );
    assert!(!text.contains("work ended without a result"), "{text}");
}
