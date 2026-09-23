//! RC: the upgrade/downgrade matrix runtime × artifact. With
//! `USAI_PREVIOUS_BIN` pointing at the previous release's binary (CI
//! downloads it), an artifact built by that release must serve on this
//! runtime (upgrade), and an artifact built by this runtime must serve on
//! the previous one (downgrade) — the manifest format has not changed. When
//! the format does change, the refused direction must be refused *before*
//! anything listens, with the compatibility message. Skips without the env.
mod support;

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn serves(bin: &str, artifact: &std::path::Path) -> Result<(), String> {
    let port = support::free_port();
    let mut child = Command::new(bin)
        .args([
            "--root",
            "/",
            "run",
            "--artifact",
            artifact.to_str().unwrap(),
            "--port",
            &port.to_string(),
        ])
        .stderr(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    let log = support::capture(child.stderr.take().unwrap());
    let deadline = Instant::now() + Duration::from_secs(90);
    loop {
        if support::get(port, "/hello/x").map(|(s, _)| s) == Some(200) {
            unsafe { libc::kill(child.id() as i32, libc::SIGTERM) };
            let _ = child.wait();
            return Ok(());
        }
        if let Some(status) = child.try_wait().unwrap() {
            return Err(format!("exited with {status}:\n{}", log.lock().unwrap()));
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            return Err(format!(
                "did not serve within 90 s:\n{}",
                log.lock().unwrap()
            ));
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

#[test]
fn artifacts_cross_runtime_versions_in_both_directions_or_are_refused_early() {
    let Ok(previous) = std::env::var("USAI_PREVIOUS_BIN") else {
        eprintln!("skipping: set USAI_PREVIOUS_BIN to the previous release's usai binary");
        return;
    };
    if !support::node_available() {
        return;
    }
    let app = std::fs::read_to_string(support::repo().join("examples/hello/src/app.ts")).unwrap();
    let Some(dir) = support::project("matrix", &app) else {
        eprintln!("skipping: run pnpm install first");
        return;
    };
    let current = env!("CARGO_BIN_EXE_usai");
    let version = |bin: &str| {
        String::from_utf8_lossy(&Command::new(bin).arg("--version").output().unwrap().stdout)
            .trim()
            .to_owned()
    };
    eprintln!(
        "previous: {} / current: {}",
        version(&previous),
        version(current)
    );

    let build = |bin: &str, out: &str| {
        let status = Command::new(bin)
            .args(["--root", dir.to_str().unwrap(), "build"])
            .env("USAI_OUT_DIR", out)
            .status()
            .unwrap();
        assert!(status.success(), "{bin} could not build");
        // The build writes to the project's .usai/build; snapshot it aside.
        let snapshot = dir.join(out);
        let _ = std::fs::remove_dir_all(&snapshot);
        copy_dir(&dir.join(".usai/build"), &snapshot);
        snapshot
    };
    let built_by_previous = build(&previous, "built-by-previous");
    let built_by_current = build(current, "built-by-current");

    // Upgrade: yesterday's artifact on today's runtime.
    serves(current, &built_by_previous)
        .expect("an artifact built by the previous release serves on this runtime");
    // Downgrade: today's artifact on yesterday's runtime — same format, so
    // it serves (extra files such as migrations/ and app.js.map are ignored).
    // If a format bump ever makes this fail, it must fail with the
    // compatibility message and before listening.
    match serves(&previous, &built_by_current) {
        Ok(()) => {}
        Err(detail) => assert!(
            detail.contains("manifest format") && detail.contains("Rebuild the artifact"),
            "downgrade failed for another reason than the compatibility contract:\n{detail}"
        ),
    }
}

fn copy_dir(from: &std::path::Path, to: &std::path::Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap().flatten() {
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// The third axis the RC gate names and the matrix above does not cover: the
/// **SDK**. Both builds there link the workspace SDK, so they prove nothing
/// about an application written against a published one. This installs the
/// previous release's SDK from the registry, builds that project with *this*
/// runtime, and serves it — the shape of every upgrade where the operator
/// moves the runtime before the team moves its dependency.
#[test]
fn an_application_on_the_previous_published_sdk_serves_on_this_runtime() {
    let Ok(sdk_version) = std::env::var("USAI_PREVIOUS_SDK") else {
        eprintln!("skipping: set USAI_PREVIOUS_SDK to the previous release's SDK version");
        return;
    };
    if !support::node_available() {
        return;
    }
    let dir = std::env::temp_dir().join(format!("usai-sdk-matrix-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("package.json"),
        r#"{ "name": "sdk-matrix", "private": true, "type": "module" }"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("usai.config.ts"),
        "import { defineConfig } from \"@sakaladev/usai/config\";\nexport default defineConfig({ app: \"./src/app.ts\" });\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("src/app.ts"),
        std::fs::read_to_string(support::repo().join("examples/hello/src/app.ts")).unwrap(),
    )
    .unwrap();
    let install = Command::new("npm")
        .args([
            "install",
            "--no-audit",
            "--no-fund",
            &format!("@sakaladev/usai@{sdk_version}"),
            "zod",
        ])
        .current_dir(&dir)
        .status()
        .unwrap();
    assert!(
        install.success(),
        "could not install @sakaladev/usai@{sdk_version}"
    );

    let current = env!("CARGO_BIN_EXE_usai");
    let build = Command::new(current)
        .args(["--root", dir.to_str().unwrap(), "build", "--no-typecheck"])
        .output()
        .unwrap();
    assert!(
        build.status.success(),
        "this runtime could not build an application on SDK {sdk_version}:\n{}",
        String::from_utf8_lossy(&build.stderr)
    );
    serves(current, &dir.join(".usai/build")).unwrap_or_else(|detail| {
        // A guest-ABI bump is allowed to refuse it — but it must say so, and
        // before anything listens, the same contract the artifact axis has.
        assert!(
            detail.contains("guest ABI") || detail.contains("Rebuild"),
            "an application on SDK {sdk_version} neither served nor was refused with the compatibility message:\n{detail}"
        );
    });
    let _ = std::fs::remove_dir_all(&dir);
}
