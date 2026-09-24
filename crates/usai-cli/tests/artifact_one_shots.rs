//! A production image carries `.usai/build` and **no source tree** — that is
//! the point of it — so `usai app <name>` inside one answered "not a Usai
//! project: no package.json or usai.config.ts here", and the only route to a
//! declared command in production was the control surface of a *serving*
//! replica. The `db` verbs have taken `--artifact` since D5; the one-shots
//! that run application code did not (round 24).
use std::path::{Path, PathBuf};
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

fn usai(args: &[&str], cwd: &Path) -> (bool, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_usai"))
        .args(args)
        .current_dir(cwd)
        .env("UPSTREAM_URL", "http://127.0.0.1:9/")
        .output()
        .expect("usai runs");
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    (out.status.success(), text)
}

#[test]
fn a_command_runs_from_an_artifact_with_no_source_tree() {
    let Some(project) = fixture() else { return };
    // Build, then copy just the artifact into an empty directory: the shape
    // the scaffold's Dockerfile produces.
    let (ok, text) = usai(
        &["--root", project.to_str().unwrap(), "build"],
        Path::new("."),
    );
    assert!(ok, "the fixture builds: {text}");
    let image = std::env::temp_dir().join(format!("usai-image-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&image);
    std::fs::create_dir_all(&image).unwrap();
    let build = image.join("build");
    let status = Command::new("cp")
        .args(["-r", project.join(".usai/build").to_str().unwrap()])
        .arg(&build)
        .status()
        .expect("cp runs");
    assert!(status.success());

    // Without `--artifact` it is not a project, and says so.
    let (ok, text) = usai(&["app", "reconcile"], &image);
    assert!(!ok, "{text}");
    assert!(text.contains("not a Usai project"), "{text}");

    // With it, the command runs.
    let (ok, text) = usai(
        &["app", "reconcile", "--artifact", build.to_str().unwrap()],
        &image,
    );
    assert!(ok, "a command must run from an artifact alone: {text}");
    assert!(text.contains("args"), "{text}");
    let _ = std::fs::remove_dir_all(&image);
}
