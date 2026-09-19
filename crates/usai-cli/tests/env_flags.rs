//! The documented environment forms of the boolean flags (`USAI_NO_CRON=1`,
//! `USAI_DIAGNOSTICS=1`, …) must be accepted: clap's default parser for a
//! `bool` taken from the environment accepted only `true`/`false`, so every
//! compose file that followed the docs would have failed at start.
use std::process::Command;

#[test]
fn boolean_flags_accept_the_documented_environment_forms() {
    // `0`, `false`, `no`, `off` and the empty string are off; anything else
    // is on (clap's falsey parser), so a typo turns a flag on rather than
    // refusing to start — the safer direction for `--no-*` switches.
    for (var, value, ok) in [
        ("USAI_NO_CRON", "1", true),
        ("USAI_NO_QUEUE", "true", true),
        ("USAI_NO_SERVICES", "yes", true),
        ("USAI_DIAGNOSTICS", "0", true),
        ("USAI_DIAGNOSTICS", "", true),
    ] {
        // `run --help` parses the arguments (and the environment) without
        // starting anything.
        let out = Command::new(env!("CARGO_BIN_EXE_usai"))
            .env(var, value)
            .args(["run", "--help"])
            .output()
            .unwrap();
        assert_eq!(
            out.status.success(),
            ok,
            "{var}={value:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}
