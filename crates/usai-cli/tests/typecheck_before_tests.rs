//! `usai build` typechecked and `usai test` did not, so a suite could be
//! green over code that does not compile — and the mistakes TypeScript
//! catches here are the ones the runtime cannot see at all: an option key
//! that is not in the declaration's shape is dropped by the SDK, so
//! `retry: { delay: "100ms" }` where the field is `baseMs` runs the declared
//! default and nothing says a word (round 23).
//!
//! The check covers **both** projects. A test file lives outside the
//! application's own `tsconfig.json` — it imports `node:test`, which a world
//! does not have — so checking only that one leaves every test unchecked.
use std::path::{Path, PathBuf};
use std::process::Command;

/// The fixture, with `node_modules/typescript` linked to this repository's
/// copy: the check is skipped entirely when a project has no typescript, so
/// without the link this test would pass for the wrong reason.
fn project() -> Option<PathBuf> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/typecheck-app")
        .canonicalize()
        .ok()?;
    let tsc = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../node_modules/typescript")
        .canonicalize()
        .ok()?;
    let modules = root.join("node_modules");
    let link = modules.join("typescript");
    if !link.exists() {
        std::fs::create_dir_all(&modules).ok()?;
        std::os::unix::fs::symlink(&tsc, &link).ok()?;
    }
    Some(root)
}

fn run(root: &Path, args: &[&str]) -> (bool, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_usai"))
        .args(["--root", root.to_str().expect("utf-8 path"), "test"])
        .args(args)
        .output()
        .expect("usai runs");
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    (out.status.success(), text)
}

#[test]
fn a_type_error_in_a_test_file_stops_the_run() {
    let Some(root) = project() else { return };
    let (ok, text) = run(&root, &[]);
    assert!(!ok, "a type error should stop the run:\n{text}");
    assert!(
        text.contains("TypeScript errors"),
        "the failure must say what it was:\n{text}"
    );
    assert!(
        text.contains("broken.test.ts"),
        "the error is in a test file, which the application's tsconfig excludes:\n{text}"
    );
    // And it is skippable, which is how `usai build` already behaves. The
    // run then fails for its own reasons (this fixture has no application to
    // build) — what matters is that it got past the check.
    let (_, skipped) = run(&root, &["--no-typecheck"]);
    assert!(
        !skipped.contains("TypeScript errors"),
        "--no-typecheck must skip the check:\n{skipped}"
    );
}
