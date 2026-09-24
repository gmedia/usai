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

    // A command's own arguments are variadic and may contain hyphens, so
    // everything after the first of them is trailing — and `--artifact`
    // written there is handed to the command instead of to usai. What came
    // back was "not a Usai project", hinting about `--root` and scaffolding:
    // the exact error this flag exists to remove, for an invocation an
    // operator has every reason to write.
    let (ok, text) = usai(
        &[
            "app",
            "reconcile",
            "since=2026-01-01",
            "--artifact",
            build.to_str().unwrap(),
        ],
        &image,
    );
    assert!(!ok, "{text}");
    assert!(
        !text.contains("not a Usai project"),
        "the flag was swallowed and the error blames the directory: {text}"
    );
    assert!(
        text.contains("--artifact") && text.contains("put it first"),
        "the refusal has to say where the flag goes: {text}"
    );
    // And the order it suggests works.
    let (ok, text) = usai(
        &[
            "app",
            "--artifact",
            build.to_str().unwrap(),
            "reconcile",
            "since=2026-01-01",
        ],
        &image,
    );
    assert!(ok, "the suggested order has to work: {text}");

    // **Which build is in this directory?** After a control-plane deploy the
    // running revision and the `--artifact` the process was started with
    // disagree, and `CONTROL-API.md` correctly warns that any restart brings
    // back whatever the directory holds — but there was no way to check that
    // you had rewritten it. `/_usai/status` carries the *revision's*
    // identity and the artifact carried none, so the two halves of the
    // comparison existed and never met. An on-call round demonstrated the
    // cost end to end: a fix deployed through the control plane, a restart
    // on the same command line, and the break back with every probe green.
    //
    // `usai inspect --root` was the only thing that printed an identity, and
    // a production image is exactly where there is no source tree to point
    // it at.
    let (ok, text) = usai(&["inspect", "--artifact", build.to_str().unwrap()], &image);
    assert!(ok, "inspect must read an artifact with no project: {text}");
    let identity = text
        .lines()
        .find_map(|l| l.strip_prefix("Identity:"))
        .map(str::trim)
        .unwrap_or_default()
        .to_owned();
    assert_eq!(
        identity.len(),
        16,
        "the artifact's identity is what `/_usai/status` prints for the revision: {text}"
    );
    // And it is the *same* identity the runtime computes, or the comparison
    // an operator is being told to make would be between two different
    // things.
    let (ok, from_project) = usai(
        &["--root", project.to_str().unwrap(), "inspect"],
        Path::new("."),
    );
    assert!(ok, "{from_project}");
    assert!(
        from_project.contains(&identity),
        "the artifact and the project it was built from disagree about identity:\n{from_project}"
    );
    let _ = std::fs::remove_dir_all(&image);
}
