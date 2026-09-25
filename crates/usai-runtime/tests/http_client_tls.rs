//! Outbound TLS: a private certificate authority, and a client certificate.
//!
//! `postgres` has taken `tls: { caFile }` since it shipped and `httpClient`
//! took nothing, so a deployment that had to call a router's REST API or a
//! mutually-authenticated internal service had no option but a TLS-terminating
//! proxy in the path. That was an asymmetry rather than a decision.
//!
//! The point of these tests is that the trust is **used**: the same request
//! that succeeds with `caFile` fails without it, against the same server.
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::Arc;

use serde_json::json;
use tokio_util::sync::CancellationToken;
use usai_runtime::definition::ResourceSpec;
use usai_runtime::resource::{ResourceCall, ResourceIdentity, ResourceProvider};

/// A private CA and a `localhost` certificate it signs, written to a
/// temporary directory. `None` when openssl is not available.
fn certs() -> Option<PathBuf> {
    let dir = std::env::temp_dir().join(format!("usai-httptls-{}", std::process::id()));
    if dir.join("server.crt").exists() {
        return Some(dir);
    }
    std::fs::create_dir_all(&dir).ok()?;
    let p = |n: &str| dir.join(n).to_str().unwrap().to_owned();
    let run = |args: &[&str]| -> Option<bool> {
        Some(
            std::process::Command::new("openssl")
                .args(args)
                .output()
                .ok()?
                .status
                .success(),
        )
    };
    if !run(&[
        "req",
        "-x509",
        "-newkey",
        "rsa:2048",
        "-nodes",
        "-days",
        "2",
        "-subj",
        "/CN=usai-test-ca",
        "-addext",
        "basicConstraints=critical,CA:TRUE",
        "-addext",
        "keyUsage=critical,keyCertSign,cRLSign",
        "-keyout",
        &p("ca.key"),
        "-out",
        &p("ca.crt"),
    ])? {
        return None;
    }
    if !run(&[
        "req",
        "-newkey",
        "rsa:2048",
        "-nodes",
        "-subj",
        "/CN=localhost",
        "-keyout",
        &p("server.key"),
        "-out",
        &p("server.csr"),
    ])? {
        return None;
    }
    std::fs::write(
        dir.join("server.ext"),
        "subjectAltName=DNS:localhost,IP:127.0.0.1\nbasicConstraints=CA:FALSE\nkeyUsage=digitalSignature,keyEncipherment\nextendedKeyUsage=serverAuth\n",
    )
    .ok()?;
    if !run(&[
        "x509",
        "-req",
        "-days",
        "2",
        "-in",
        &p("server.csr"),
        "-CA",
        &p("ca.crt"),
        "-CAkey",
        &p("ca.key"),
        "-CAcreateserial",
        "-extfile",
        &p("server.ext"),
        "-out",
        &p("server.crt"),
    ])? {
        return None;
    }
    Some(dir)
}

/// A one-shot HTTPS server on loopback. Returns its port; it answers every
/// connection with the same tiny JSON body and then closes.
fn serve_https(dir: &std::path::Path) -> u16 {
    let certs: Vec<_> = rustls_pemfile::certs(&mut std::io::BufReader::new(
        std::fs::File::open(dir.join("server.crt")).unwrap(),
    ))
    .collect::<Result<_, _>>()
    .unwrap();
    let key = rustls_pemfile::private_key(&mut std::io::BufReader::new(
        std::fs::File::open(dir.join("server.key")).unwrap(),
    ))
    .unwrap()
    .unwrap();
    let mut config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .unwrap();
    // Keep the test's server simple: one protocol, one request, one reply.
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    let config = Arc::new(config);

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let config = Arc::clone(&config);
            std::thread::spawn(move || {
                let Ok(conn) = rustls::ServerConnection::new(config) else {
                    return;
                };
                let mut tls = rustls::StreamOwned::new(conn, stream);
                let mut buf = [0u8; 1024];
                let _ = tls.read(&mut buf);
                let body = br#"{"ok":true}"#;
                let _ = tls.write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                        body.len()
                    )
                    .as_bytes(),
                );
                let _ = tls.write_all(body);
                let _ = tls.flush();
            });
        }
    });
    port
}

async fn fetch_through(spec: ResourceSpec, url: &str) -> Result<serde_json::Value, String> {
    let provider = usai_runtime::resource::http_client::HttpClientProvider;
    let env = |_: &str| None;
    let identity = ResourceIdentity::compute(&spec, &env, provider.compat());
    let manager = provider
        .open(&spec, identity, &env)
        .await
        .map_err(|e| format!("activation: {e}"))?;
    manager
        .call(
            ResourceCall {
                method: "fetch".into(),
                args: json!({ "url": url }),
            },
            CancellationToken::new(),
        )
        .await
        .map_err(|e| format!("{}: {e}", e.code()))
}

fn spec(name: &str, config: serde_json::Value) -> ResourceSpec {
    ResourceSpec {
        name: name.into(),
        kind: "http.client".into(),
        module: None,
        config,
        env: Vec::new(),
    }
}

/// The same request against the same server, with and without the CA: one
/// succeeds and one does not. Without the negative half this would pass on a
/// client that ignored `caFile` entirely.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_private_certificate_authority_is_trusted_only_when_declared() {
    let Some(dir) = certs() else {
        eprintln!("skipping: openssl is not available");
        return;
    };
    let port = serve_https(&dir);
    let url = format!("https://localhost:{port}/status");
    let ca = dir.join("ca.crt").to_str().unwrap().to_owned();

    let trusted = fetch_through(
        spec(
            "with-ca",
            json!({ "baseUrl": url, "tls": { "caFile": ca }, "allowPrivateNetwork": true }),
        ),
        &url,
    )
    .await
    .expect("a declared CA has to be trusted");
    assert_eq!(trusted["status"], 200, "{trusted}");

    let untrusted = fetch_through(
        spec(
            "no-ca",
            json!({ "baseUrl": url, "allowPrivateNetwork": true }),
        ),
        &url,
    )
    .await;
    let error = untrusted.expect_err("a self-signed certificate must not be trusted by default");
    assert!(
        error.starts_with("http_"),
        "the refusal is the connection's, not ours: {error}"
    );
}

/// A certificate is read when the resource is opened, so a path that is
/// wrong stops the deployment (`GOAL.md` §32) rather than surfacing as a
/// failed request hours later.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_certificate_that_cannot_be_read_fails_activation() {
    let missing = fetch_through(
        spec(
            "missing",
            json!({ "tls": { "caFile": "/nonexistent/ca.pem" } }),
        ),
        "https://example.invalid/",
    )
    .await
    .expect_err("a missing CA file must fail activation");
    assert!(
        missing.contains("activation") && missing.contains("caFile"),
        "the failure has to name the option and the path: {missing}"
    );

    // And half an mTLS pair is a configuration mistake, not a silent
    // fallback to no client certificate.
    let Some(dir) = certs() else { return };
    let half = fetch_through(
        spec(
            "half",
            json!({ "tls": { "clientCertFile": dir.join("server.crt").to_str().unwrap() } }),
        ),
        "https://example.invalid/",
    )
    .await
    .expect_err("a client certificate without its key must fail activation");
    assert!(
        half.contains("clientKeyFile"),
        "the failure has to say what is missing: {half}"
    );
}
