//! Support for the CLI acceptance tests: a private project directory built
//! from `examples/hello`'s package layout, and a raw HTTP GET.
#![allow(dead_code)]

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

pub fn node_available() -> bool {
    Command::new("node")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

pub fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// A temp project with `examples/hello`'s package.json and usai.config.ts,
/// the given `src/app.ts`, and `node_modules` linked to the workspace SDK
/// and zod. `None` when the workspace has not been `pnpm install`ed.
pub fn project(tag: &str, app_ts: &str) -> Option<PathBuf> {
    let hello = repo().join("examples/hello");
    let zod = std::fs::canonicalize(hello.join("node_modules/zod")).ok()?;
    let usai = std::fs::canonicalize(repo().join("packages/usai")).ok()?;
    let dir = std::env::temp_dir().join(format!("usai-cli-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).ok()?;
    for f in ["package.json", "usai.config.ts"] {
        std::fs::copy(hello.join(f), dir.join(f)).ok()?;
    }
    std::fs::write(dir.join("src/app.ts"), app_ts).ok()?;
    link(&dir.join("node_modules/usai"), &usai);
    link(&dir.join("node_modules/zod"), &zod);
    Some(dir)
}

fn link(from: &Path, to: &Path) {
    std::fs::create_dir_all(from.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(to, from).unwrap();
}

pub fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

pub fn get(port: u16, path: &str) -> Option<(u16, String)> {
    let mut s = TcpStream::connect(("127.0.0.1", port)).ok()?;
    s.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
    write!(
        s,
        "GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"
    )
    .ok()?;
    let mut raw = String::new();
    s.read_to_string(&mut raw).ok()?;
    let status: u16 = raw.split_whitespace().nth(1)?.parse().ok()?;
    let body = raw.split("\r\n\r\n").nth(1).unwrap_or("").to_owned();
    Some((status, body))
}

/// Collects a child's stderr on a thread.
pub fn capture(stderr: std::process::ChildStderr) -> std::sync::Arc<std::sync::Mutex<String>> {
    let log = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
    let writer = std::sync::Arc::clone(&log);
    let mut stderr = stderr;
    std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        while let Ok(n) = stderr.read(&mut buf) {
            if n == 0 {
                break;
            }
            writer
                .lock()
                .unwrap()
                .push_str(&String::from_utf8_lossy(&buf[..n]));
        }
    });
    log
}

pub fn wait_for_log(log: &std::sync::Mutex<String>, needle: &str, timeout: Duration) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        if log.lock().unwrap().contains(needle) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    false
}
