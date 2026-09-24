//! `usai inspect` is the one source of truth about what the runtime
//! understood (C8), so it must not describe a boundary the runtime does not
//! have. It printed "validated before world creation" for **every** declared
//! contract, including a task's `input` and a socket's `message` — which are
//! parsed in the world, because no host-side validator exists for those
//! kinds — and then explained the in-world parse as "the schema transforms
//! or refines" about schemas that do neither.
use std::path::PathBuf;
use std::process::Command;

fn fixture() -> Option<PathBuf> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../usai-runtime/tests/fixtures/http-app")
        .canonicalize()
        .ok()?;
    root.join("node_modules/@sakaladev/usai")
        .exists()
        .then_some(root)
}

#[test]
fn inspect_says_where_each_contract_is_actually_checked() {
    let Some(root) = fixture() else { return };
    let out = Command::new(env!("CARGO_BIN_EXE_usai"))
        .args(["--root", root.to_str().unwrap(), "inspect"])
        .output()
        .expect("usai inspect runs");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8_lossy(&out.stdout);
    let tasks = text
        .split_once("\nTasks\n")
        .map(|(_, rest)| rest.split("\n\n").next().unwrap_or(rest).to_owned())
        .unwrap_or_default();
    assert!(
        tasks.contains("input:"),
        "the fixture has a task with a declared input: {tasks}"
    );
    assert!(
        !tasks.contains("input: validated before world creation"),
        "a task's input is parsed in the world; the host has no validator for it:\n{tasks}"
    );
    assert!(
        tasks.contains("input: validated in the world (this kind has no host-side boundary)"),
        "{tasks}"
    );
    // And the kind that *does* have one still says so.
    assert!(
        text.contains("body: validated before world creation"),
        "an HTTP route's body is checked before a world exists"
    );
}
