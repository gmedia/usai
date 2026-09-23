//! Where a developer's `console.log` goes — the first thing anyone does to
//! debug, and it used to be the one place logging was refused.
//!
//! A `console.log` at module scope runs while the application is being
//! *defined* (the module is evaluated to read what it declares, ADR-0009).
//! The bridge routes it through the host's control call, and the build phase
//! counted every control call as an operation — so the build failed with
//! "declarations must not perform I/O", which is true of the counter and
//! wrong about logging. In `usai dev` the failed rebuild meant the previous
//! revision kept serving, so it looked exactly like "my log did not appear".
mod support;

use std::process::Command;

const LOGGING_APP: &str = r#"
import { defineApp, http } from "@sakaladev/usai";
console.log("DEFINITION-TIME line", { where: "module scope" });
console.warn("DEFINITION-TIME warning");
export const hello = http.get("/hello/:name", {}, async (ctx) => {
  console.log("REQUEST-TIME line", { name: ctx.params.name });
  return { hello: ctx.params.name };
});
export default defineApp({ name: "logging", workloads: [hello] });
"#;

#[test]
fn console_log_in_a_declaration_builds_and_is_printed() {
    if !support::node_available() {
        return;
    }
    let Some(dir) = support::project("console", LOGGING_APP) else {
        eprintln!("skipping: run pnpm install first");
        return;
    };
    let out = Command::new(env!("CARGO_BIN_EXE_usai"))
        .args(["--root", dir.to_str().unwrap(), "build", "--no-typecheck"])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "a console.log in a declaration must not fail the build:\n{stderr}"
    );
    assert!(
        stderr.contains("DEFINITION-TIME line"),
        "the line the developer wrote must be printed:\n{stderr}"
    );
    assert!(
        stderr.contains("DEFINITION-TIME warning"),
        "console.warn too:\n{stderr}"
    );
    // The structured fields ride along, the same way a handler's do.
    assert!(
        stderr.contains("module scope"),
        "the trailing object is the line's fields:\n{stderr}"
    );
    // And a declaration that does real I/O is still refused: the counter has
    // not been turned off, only taught what an operation is.
    let io_app = LOGGING_APP.replace(
        r#"console.log("DEFINITION-TIME line", { where: "module scope" });"#,
        "await new Promise((r) => setTimeout(r, 1));",
    );
    let Some(io_dir) = support::project("console-io", &io_app) else {
        return;
    };
    let refused = Command::new(env!("CARGO_BIN_EXE_usai"))
        .args([
            "--root",
            io_dir.to_str().unwrap(),
            "build",
            "--no-typecheck",
        ])
        .output()
        .unwrap();
    assert!(
        !refused.status.success(),
        "a declaration that performs I/O must still be refused"
    );
}
