//! D3 acceptance: `usai dev` rebuilds on edit and swaps revisions without a
//! failed request. Drives the real binary against a copy of `examples/hello`.
mod support;

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[test]
fn editing_a_handler_reloads_without_a_failed_request() {
    if !support::node_available() {
        eprintln!("skipping: node is not available");
        return;
    }
    let original =
        std::fs::read_to_string(support::repo().join("examples/hello/src/app.ts")).unwrap();
    let Some(dir) = support::project("dev-reload", &original) else {
        eprintln!("skipping: run pnpm install first");
        return;
    };
    let app = dir.join("src/app.ts");
    let port = support::free_port();
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
    let log = support::capture(child.stderr.take().unwrap());
    let stop = |child: &mut std::process::Child| {
        unsafe { libc::kill(child.id() as i32, libc::SIGINT) };
        let _ = child.wait();
    };

    // First revision serves.
    let deadline = Instant::now() + Duration::from_secs(120);
    loop {
        if let Some((200, body)) = support::get(port, "/hello/x") {
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
        let (status, body) =
            support::get(port, "/hello/x").expect("server stays reachable during reload");
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
    assert!(
        support::wait_for_log(&log, "build failed", Duration::from_secs(60)),
        "{}",
        log.lock().unwrap()
    );
    let (status, body) = support::get(port, "/hello/x").unwrap();
    assert_eq!(status, 200);
    assert!(body.contains("edited-x"), "{body}");

    stop(&mut child);
    let _ = std::fs::remove_dir_all(&dir);
}
