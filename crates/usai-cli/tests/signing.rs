//! Production gate: a runtime that requires signatures serves only artifacts
//! signed by a key it trusts, refuses everything else before listening, and
//! the control surface applies the same rule to installs.
mod support;

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn usai() -> Command {
    Command::new(env!("CARGO_BIN_EXE_usai"))
}

fn run_with(
    artifact: &std::path::Path,
    trust: &str,
) -> (Option<u16>, String, Option<std::process::Child>) {
    let port = support::free_port();
    let mut child = usai()
        .args([
            "--root",
            "/",
            "run",
            "--artifact",
            artifact.to_str().unwrap(),
            "--port",
            &port.to_string(),
            "--require-signature",
            trust,
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let log = support::capture(child.stderr.take().unwrap());
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        if let Some((200, _)) = support::get(port, "/hello/x") {
            return (Some(port), log.lock().unwrap().clone(), Some(child));
        }
        if child.try_wait().unwrap().is_some() {
            std::thread::sleep(Duration::from_millis(100));
            return (None, log.lock().unwrap().clone(), None);
        }
        assert!(Instant::now() < deadline, "no decision within 60 s");
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
fn signed_artifacts_only_when_a_signature_is_required() {
    if !support::node_available() {
        return;
    }
    let app = std::fs::read_to_string(support::repo().join("examples/hello/src/app.ts")).unwrap();
    let Some(dir) = support::project("signing", &app) else {
        eprintln!("skipping: run pnpm install first");
        return;
    };
    let key = dir.join("signing.key");
    let out = usai()
        .args(["keygen", "--out", key.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let public = String::from_utf8_lossy(&out.stdout)
        .lines()
        .find_map(|l| l.strip_prefix("public key:").map(|k| k.trim().to_owned()))
        .unwrap();
    let other = usai()
        .args(["keygen", "--out", dir.join("other.key").to_str().unwrap()])
        .output()
        .unwrap();
    let other_public = String::from_utf8_lossy(&other.stdout)
        .lines()
        .find_map(|l| l.strip_prefix("public key:").map(|k| k.trim().to_owned()))
        .unwrap();

    // Unsigned artifact: refused before listening, with the fix.
    let build = usai()
        .args(["--root", dir.to_str().unwrap(), "build", "--no-typecheck"])
        .output()
        .unwrap();
    assert!(
        build.status.success(),
        "{}",
        String::from_utf8_lossy(&build.stderr)
    );
    let artifact = dir.join(".usai/build");
    let (port, log, _) = run_with(&artifact, &public);
    assert!(port.is_none(), "an unsigned artifact must not serve");
    assert!(log.contains("usai build --sign"), "{log}");

    // Signed: serves.
    let build = usai()
        .args([
            "--root",
            dir.to_str().unwrap(),
            "build",
            "--no-typecheck",
            "--sign",
            key.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        build.status.success(),
        "{}",
        String::from_utf8_lossy(&build.stderr)
    );
    assert!(artifact.join("signature.json").exists());
    let (port, _, child) = run_with(&artifact, &public);
    assert!(port.is_some(), "a signed artifact serves");
    let mut child = child.unwrap();
    unsafe { libc::kill(child.id() as i32, libc::SIGTERM) };
    let _ = child.wait();

    // Signed by a key the runtime does not trust: refused.
    let (port, log, _) = run_with(&artifact, &other_public);
    assert!(port.is_none());
    assert!(log.contains("does not trust"), "{log}");

    // Tampered native image after signing: refused.
    let image = artifact.join("cache/image.cwasm");
    if image.exists() {
        let mut bytes = std::fs::read(&image).unwrap();
        bytes[100] ^= 0xff;
        std::fs::write(&image, bytes).unwrap();
        let (port, log, _) = run_with(&artifact, &public);
        assert!(port.is_none(), "a modified native image must not load");
        assert!(log.contains("modified after signing"), "{log}");
    }

    // Trust list from a file with two keys works as well.
    let trust_file = dir.join("trusted.txt");
    std::fs::write(&trust_file, format!("# ops\n{other_public}\n{public}\n")).unwrap();
    let build = usai()
        .args([
            "--root",
            dir.to_str().unwrap(),
            "build",
            "--no-typecheck",
            "--sign",
            key.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(build.status.success());
    let (port, _, child) = run_with(&artifact, trust_file.to_str().unwrap());
    assert!(port.is_some());
    let mut child = child.unwrap();
    unsafe { libc::kill(child.id() as i32, libc::SIGTERM) };
    let _ = child.wait();
}
