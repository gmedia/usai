//! D3 acceptance: `usai dev` rebuilds on edit and swaps revisions without a
//! failed request. Drives the real binary against a copy of `examples/hello`.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn get(port: u16, path: &str) -> Option<(u16, String)> {
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

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn link(from: &Path, to: &Path) {
    std::fs::create_dir_all(from.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(to, from).unwrap();
}

#[test]
fn editing_a_handler_reloads_without_a_failed_request() {
    if Command::new("node")
        .arg("--version")
        .output()
        .map(|o| !o.status.success())
        .unwrap_or(true)
    {
        eprintln!("skipping: node is not available");
        return;
    }
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let hello = repo.join("examples/hello");
    let Ok(zod) = std::fs::canonicalize(hello.join("node_modules/zod")) else {
        eprintln!("skipping: run pnpm install first");
        return;
    };
    let usai = std::fs::canonicalize(repo.join("packages/usai")).unwrap();

    // A private copy of the example so the edit does not touch the tree.
    let dir = std::env::temp_dir().join(format!("usai-dev-reload-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    for f in ["package.json", "usai.config.ts"] {
        std::fs::copy(hello.join(f), dir.join(f)).unwrap();
    }
    let app = dir.join("src/app.ts");
    let original = std::fs::read_to_string(hello.join("src/app.ts")).unwrap();
    std::fs::write(&app, &original).unwrap();
    link(&dir.join("node_modules/usai"), &usai);
    link(&dir.join("node_modules/zod"), &zod);

    let port = free_port();
    let mut child = Command::new(env!("CARGO_BIN_EXE_usai"))
        .args([
            "--root",
            dir.to_str().unwrap(),
            "dev",
            "--port",
            &port.to_string(),
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stderr = child.stderr.take().unwrap();
    let log = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
    let log_writer = std::sync::Arc::clone(&log);
    std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        while let Ok(n) = stderr.read(&mut buf) {
            if n == 0 {
                break;
            }
            log_writer
                .lock()
                .unwrap()
                .push_str(&String::from_utf8_lossy(&buf[..n]));
        }
    });
    let stop = |child: &mut std::process::Child| {
        unsafe { libc::kill(child.id() as i32, libc::SIGINT) };
        let _ = child.wait();
    };

    // First revision serves.
    let deadline = Instant::now() + Duration::from_secs(120);
    loop {
        if let Some((200, body)) = get(port, "/hello/x") {
            assert!(body.contains("\"hello\":\"x\""), "{body}");
            break;
        }
        assert!(
            Instant::now() < deadline,
            "dev server did not come up:\n{}",
            log.lock().unwrap()
        );
        std::thread::sleep(Duration::from_millis(200));
    }

    // Edit the handler; every request during the reload must succeed.
    let edited = original.replace(
        "return { hello: ctx.params.name };",
        "return { hello: \"edited-\" + ctx.params.name };",
    );
    assert_ne!(
        edited, original,
        "the example's handler line moved; update the test"
    );
    std::fs::write(&app, edited).unwrap();
    let deadline = Instant::now() + Duration::from_secs(120);
    let mut requests = 0;
    loop {
        let (status, body) = get(port, "/hello/x").expect("server stays reachable during reload");
        assert_eq!(
            status,
            200,
            "a request failed during reload: {body}\n{}",
            log.lock().unwrap()
        );
        requests += 1;
        if body.contains("\"hello\":\"edited-x\"") {
            break;
        }
        assert!(body.contains("\"hello\":\"x\""), "{body}");
        if Instant::now() > deadline {
            stop(&mut child);
            panic!("reload did not land:\n{}", log.lock().unwrap());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(requests > 1);
    assert!(
        log.lock().unwrap().contains("revision rev2 active"),
        "{}",
        log.lock().unwrap()
    );

    // A broken edit keeps the previous revision serving.
    std::fs::write(&app, "export default ???").unwrap();
    let deadline = Instant::now() + Duration::from_secs(60);
    while !log.lock().unwrap().contains("build failed") {
        assert!(Instant::now() < deadline, "{}", log.lock().unwrap());
        std::thread::sleep(Duration::from_millis(100));
    }
    let (status, body) = get(port, "/hello/x").unwrap();
    assert_eq!(status, 200);
    assert!(body.contains("edited-x"), "{body}");

    stop(&mut child);
    let _ = std::fs::remove_dir_all(&dir);
}
