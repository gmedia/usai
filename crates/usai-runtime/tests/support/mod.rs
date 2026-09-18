//! Test support: a PostgreSQL server for the D6 tests.
//!
//! Uses `USAI_TEST_DATABASE_URL` when set (CI service container); otherwise
//! starts a portable PostgreSQL from `~/.cache/usai/postgresql/<version>`,
//! downloading it once if needed. Returns `None` when neither is possible,
//! so the tests skip instead of failing on a bare machine.

#![allow(dead_code)]

use std::path::PathBuf;
use std::process::Command;
use std::sync::OnceLock;

const VERSION: &str = "18.6.0";
const ASSET: &str = "https://github.com/theseus-rs/postgresql-binaries/releases/download/18.6.0/postgresql-18.6.0-x86_64-unknown-linux-gnu.tar.gz";

struct Embedded {
    bin: PathBuf,
    data: PathBuf,
    port: u16,
}

static SERVER: OnceLock<Option<String>> = OnceLock::new();
static EMBEDDED: OnceLock<Embedded> = OnceLock::new();

extern "C" fn stop_embedded() {
    if let Some(pg) = EMBEDDED.get() {
        let _ = Command::new(pg.bin.join("pg_ctl"))
            .args(["-D", pg.data.to_str().unwrap(), "-m", "fast", "-w", "stop"])
            .output();
        let _ = std::fs::remove_dir_all(&pg.data);
    }
}

fn install_dir() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(
        PathBuf::from(home)
            .join(".cache/usai/postgresql")
            .join(VERSION),
    )
}

fn ensure_binaries() -> Option<PathBuf> {
    let dir = install_dir()?;
    if dir.join("bin/postgres").exists() {
        return Some(dir);
    }
    if !cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        return None;
    }
    eprintln!(
        "downloading portable PostgreSQL {VERSION} to {}",
        dir.display()
    );
    std::fs::create_dir_all(&dir).ok()?;
    let archive = dir.join("pg.tgz");
    let ok = Command::new("curl")
        .args(["-sSL", "-o"])
        .arg(&archive)
        .arg(ASSET)
        .status()
        .ok()?
        .success();
    if !ok {
        return None;
    }
    let ok = Command::new("tar")
        .args(["-xzf"])
        .arg(&archive)
        .arg("-C")
        .arg(&dir)
        .arg("--strip-components=1")
        .status()
        .ok()?
        .success();
    let _ = std::fs::remove_file(&archive);
    ok.then_some(dir)
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn start_embedded() -> Option<String> {
    let bin = ensure_binaries()?.join("bin");
    let data = std::env::temp_dir().join(format!("usai-pg-{}", std::process::id()));
    let ok = Command::new(bin.join("initdb"))
        .args([
            "-D",
            data.to_str()?,
            "-U",
            "usai",
            "--auth=trust",
            "-E",
            "UTF8",
        ])
        .output()
        .ok()?
        .status
        .success();
    if !ok {
        return None;
    }
    let port = free_port();
    let socket_dir = data.to_str()?.to_owned();
    let ok = Command::new(bin.join("pg_ctl"))
        .args([
            "-D",
            data.to_str()?,
            "-w",
            "-l",
            data.join("pg.log").to_str()?,
            "-o",
        ])
        .arg(format!(
            "-p {port} -k {socket_dir} -c listen_addresses=127.0.0.1 -c max_connections=100"
        ))
        .arg("start")
        .output()
        .ok()?
        .status
        .success();
    if !ok {
        return None;
    }
    EMBEDDED
        .set(Embedded {
            bin: bin.clone(),
            data,
            port,
        })
        .ok()?;
    unsafe {
        libc::atexit(stop_embedded);
    }
    Some(format!("postgres://usai@127.0.0.1:{port}/postgres"))
}

static TLS_SERVER: OnceLock<Option<(String, PathBuf)>> = OnceLock::new();
static TLS_EMBEDDED: OnceLock<Embedded> = OnceLock::new();

extern "C" fn stop_tls_embedded() {
    if let Some(pg) = TLS_EMBEDDED.get() {
        let _ = Command::new(pg.bin.join("pg_ctl"))
            .args(["-D", pg.data.to_str().unwrap(), "-m", "fast", "-w", "stop"])
            .output();
        let _ = std::fs::remove_dir_all(&pg.data);
    }
}

/// A second portable server with `ssl = on` behind a self-signed
/// certificate for `localhost`; returns `(url, ca_file)` or `None` when the
/// portable binaries or `openssl` are unavailable. Always embedded: the CI
/// service container has no TLS.
pub fn tls_database() -> Option<(String, PathBuf)> {
    TLS_SERVER
        .get_or_init(|| {
            let bin = ensure_binaries()?.join("bin");
            let data = std::env::temp_dir().join(format!("usai-pg-tls-{}", std::process::id()));
            std::fs::create_dir_all(&data).ok()?;
            // A private CA and a server certificate it signs: the shape of a
            // real deployment (managed databases ship exactly this).
            let ca_cert = data.join("ca.crt");
            let ca_key = data.join("ca.key");
            let cert = data.join("server.crt");
            let key = data.join("server.key");
            let csr = data.join("server.csr");
            let ext = data.join("server.ext");
            let run = |args: &[&str]| -> Option<bool> {
                Some(Command::new("openssl").args(args).output().ok()?.status.success())
            };
            if !run(&["req", "-x509", "-newkey", "rsa:2048", "-nodes", "-days", "2", "-subj", "/CN=usai-test-ca",
                "-addext", "basicConstraints=critical,CA:TRUE", "-addext", "keyUsage=critical,keyCertSign,cRLSign",
                "-keyout", ca_key.to_str()?, "-out", ca_cert.to_str()?])? {
                return None;
            }
            if !run(&["req", "-newkey", "rsa:2048", "-nodes", "-subj", "/CN=localhost",
                "-keyout", key.to_str()?, "-out", csr.to_str()?])? {
                return None;
            }
            std::fs::write(&ext, "subjectAltName=DNS:localhost,IP:127.0.0.1\nbasicConstraints=CA:FALSE\nkeyUsage=digitalSignature,keyEncipherment\nextendedKeyUsage=serverAuth\n").ok()?;
            if !run(&["x509", "-req", "-days", "2", "-in", csr.to_str()?, "-CA", ca_cert.to_str()?, "-CAkey", ca_key.to_str()?,
                "-CAcreateserial", "-extfile", ext.to_str()?, "-out", cert.to_str()?])? {
                return None;
            }
            let ok = Command::new(bin.join("initdb"))
                .args(["-D", data.join("db").to_str()?, "-U", "usai", "--auth=trust", "-E", "UTF8"])
                .output()
                .ok()?
                .status
                .success();
            if !ok {
                return None;
            }
            // The server refuses a key readable by others.
            std::fs::set_permissions(&key, std::os::unix::fs::PermissionsExt::from_mode(0o600)).ok()?;
            let port = free_port();
            let socket_dir = data.to_str()?.to_owned();
            let ok = Command::new(bin.join("pg_ctl"))
                .args(["-D", data.join("db").to_str()?, "-w", "-l", data.join("pg.log").to_str()?, "-o"])
                .arg(format!(
                    "-p {port} -k {socket_dir} -c listen_addresses=127.0.0.1 -c ssl=on -c ssl_cert_file={} -c ssl_key_file={}",
                    cert.display(),
                    key.display()
                ))
                .arg("start")
                .output()
                .ok()?
                .status
                .success();
            if !ok {
                eprintln!("{}", std::fs::read_to_string(data.join("pg.log")).unwrap_or_default());
                return None;
            }
            TLS_EMBEDDED
                .set(Embedded {
                    bin: bin.clone(),
                    data: data.join("db"),
                    port,
                })
                .ok()?;
            unsafe {
                libc::atexit(stop_tls_embedded);
            }
            Some((
                format!("postgres://usai@localhost:{port}/postgres?sslmode=require"),
                ca_cert,
            ))
        })
        .clone()
}

/// A connection URL to a database the tests may freely mutate, or `None`
/// to skip.
pub fn database_url() -> Option<String> {
    SERVER
        .get_or_init(|| {
            if let Ok(url) = std::env::var("USAI_TEST_DATABASE_URL") {
                return Some(url);
            }
            start_embedded()
        })
        .clone()
}

/// Creates a new database on the server and returns its URL.
pub async fn fresh_database(server_url: &str) -> String {
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let name = format!(
        "usai_t_{}_{}",
        std::process::id(),
        SEQ.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    );
    let (client, connection) = tokio_postgres::connect(server_url, tokio_postgres::NoTls)
        .await
        .expect("connect");
    tokio::spawn(connection);
    client
        .execute(&format!("create database {name}"), &[])
        .await
        .expect("create database");
    let mut parsed: tokio_postgres::Config = server_url.parse().expect("url");
    parsed.dbname(&name);
    // Rebuild a URL from the parsed config's parts.
    let host = match parsed.get_hosts().first() {
        Some(tokio_postgres::config::Host::Tcp(h)) => h.clone(),
        _ => "127.0.0.1".into(),
    };
    let port = parsed.get_ports().first().copied().unwrap_or(5432);
    let user = parsed.get_user().unwrap_or("postgres");
    let password = parsed
        .get_password()
        .map(|p| format!(":{}", String::from_utf8_lossy(p)))
        .unwrap_or_default();
    format!("postgres://{user}{password}@{host}:{port}/{name}")
}
