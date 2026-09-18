//! P3.5: an artifact this runtime cannot serve is refused before anything
//! listens, with a message that names the format, the versions that built
//! it, and the way out. `--artifact` needs no project, config or toolchain.
mod support;

use std::process::Command;

#[test]
fn an_incompatible_artifact_is_refused_before_serving_and_artifact_needs_no_project() {
    if !support::node_available() {
        return;
    }
    let original =
        std::fs::read_to_string(support::repo().join("examples/hello/src/app.ts")).unwrap();
    let Some(dir) = support::project("compat", &original) else {
        eprintln!("skipping: run pnpm install first");
        return;
    };
    let build = Command::new(env!("CARGO_BIN_EXE_usai"))
        .args(["--root", dir.to_str().unwrap(), "build"])
        .output()
        .unwrap();
    assert!(
        build.status.success(),
        "{}",
        String::from_utf8_lossy(&build.stderr)
    );
    let artifact = dir.join(".usai/build");

    // Moved out of the project: only the artifact directory, nothing else.
    let elsewhere = std::env::temp_dir().join(format!("usai-artifact-only-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&elsewhere);
    std::fs::create_dir_all(&elsewhere).unwrap();
    for entry in ["manifest.json", "app.js", "inputs.json"] {
        std::fs::copy(artifact.join(entry), elsewhere.join(entry)).unwrap();
    }
    let port = support::free_port();
    let mut child = Command::new(env!("CARGO_BIN_EXE_usai"))
        .args([
            "--root",
            "/",
            "run",
            "--artifact",
            elsewhere.to_str().unwrap(),
            "--port",
            &port.to_string(),
        ])
        .stderr(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    while support::get(port, "/hello/x").map(|(s, _)| s) != Some(200) {
        assert!(
            std::time::Instant::now() < deadline,
            "artifact-only run did not serve"
        );
        assert!(child.try_wait().unwrap().is_none(), "run exited early");
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    unsafe { libc::kill(child.id() as i32, libc::SIGTERM) };
    let status = child.wait().unwrap();
    assert!(
        status.success(),
        "SIGTERM is a graceful stop, exit {status}"
    );

    // The same artifact claiming a future format: refused, nothing listens.
    let manifest_path = elsewhere.join("manifest.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
    manifest["manifestVersion"] = serde_json::json!(99);
    manifest["builtWith"] = serde_json::json!({ "sdk": "9.9.9", "runtime": "9.9.9" });
    std::fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_usai"))
        .args([
            "--root",
            "/",
            "run",
            "--artifact",
            elsewhere.to_str().unwrap(),
            "--port",
            &port.to_string(),
        ])
        .output()
        .unwrap();
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    for needle in [
        "manifest format 99",
        "SDK 9.9.9",
        "understands format 1",
        "usai build",
    ] {
        assert!(err.contains(needle), "{err}");
    }
    assert!(
        support::get(port, "/hello/x").is_none(),
        "nothing may listen after a refusal"
    );
    let _ = std::fs::remove_dir_all(&elsewhere);
    let _ = std::fs::remove_dir_all(&dir);
}
