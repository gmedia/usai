use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context as _, Result};
use tokio_util::sync::CancellationToken;
use usai_runtime::build::{BuildOptions, build_seeder, load_artifact, load_config};
use usai_runtime::db;
use usai_runtime::http::{HttpConfig, HttpHost, serve};
use usai_runtime::*;

use crate::display;

fn engine() -> Arc<dyn usai_runtime::engine::Engine> {
    engine_with(RuntimeConfig::default().max_worlds)
}

fn engine_with(capacity: u32) -> Arc<dyn usai_runtime::engine::Engine> {
    match usai_runtime::engine::from_env(capacity) {
        Ok(engine) => engine,
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(2);
        }
    }
}

/// The project's own TypeScript check, when it has one: `tsc -p tsconfig.json
/// --noEmit` with the typescript the project installed. esbuild strips types
/// without checking them, so this is the only place a type error is caught
/// before it ships. `None` when the project has no tsconfig or no typescript.
pub async fn typecheck(root: &Path) -> Option<Result<(), String>> {
    let tsconfig = root.join("tsconfig.json");
    let tsc = root.join("node_modules/typescript/bin/tsc");
    if !tsconfig.exists() || !tsc.exists() {
        return None;
    }
    let output = tokio::process::Command::new("node")
        .arg(&tsc)
        .args(["-p", "tsconfig.json", "--noEmit", "--pretty", "false"])
        .current_dir(root)
        .output()
        .await
        .ok()?;
    if output.status.success() {
        Some(Ok(()))
    } else {
        let mut text = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        if text.is_empty() {
            text = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        }
        Some(Err(text))
    }
}

pub fn keygen(out: &Path) -> Result<()> {
    if out.exists() {
        anyhow::bail!(
            "{} exists; refusing to overwrite a signing key",
            out.display()
        );
    }
    let (key, public) = usai_runtime::signing::generate_key();
    std::fs::write(out, format!("{}\n", hex::encode(key.to_bytes())))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(out, std::fs::Permissions::from_mode(0o600))?;
    }
    println!(
        "private key: {} (keep it out of the repository)\npublic key:  {public}\n\n  usai build --sign {}\n  usai run --require-signature {public} --artifact .usai/build",
        out.display(),
        out.display()
    );
    Ok(())
}

/// Trusted signer keys from `--require-signature` values: hex keys, or
/// files with one hex key per line.
pub fn trusted_signers(values: &[String]) -> Result<Vec<ed25519_dalek::VerifyingKey>> {
    let mut keys = Vec::new();
    for value in values {
        let path = Path::new(value);
        let texts: Vec<String> = if path.exists() {
            std::fs::read_to_string(path)?
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty() && !l.starts_with('#'))
                .map(str::to_owned)
                .collect()
        } else {
            vec![value.clone()]
        };
        for text in texts {
            keys.push(
                usai_runtime::signing::parse_public_key(&text)
                    .with_context(|| format!("--require-signature {value}"))?,
            );
        }
    }
    Ok(keys)
}

pub async fn build(root: &Path, check_types: bool, sign: Option<PathBuf>) -> Result<()> {
    let engine = engine();
    let config = load_config(engine.as_ref(), root).await?;
    let started = std::time::Instant::now();
    // The type check runs alongside the bundle; both are needed for a
    // shippable artifact, so a type error fails the build (--no-typecheck
    // opts out).
    let types = async {
        if check_types {
            typecheck(root).await
        } else {
            None
        }
    };
    let options = BuildOptions::from_config(&config);
    let (out, types) = tokio::join!(usai_runtime::build::build(engine.as_ref(), &options), types);
    let out = out?;
    if let Some(Err(diagnostics)) = types {
        anyhow::bail!(
            "type check failed (the artifact was written, but do not ship it):\n{diagnostics}\n  hint: fix the errors, or pass --no-typecheck to build anyway"
        );
    }
    let signed = match sign {
        Some(key_path) => {
            let key = usai_runtime::signing::load_signing_key(&key_path)?;
            let dir = out.manifest_path.parent().expect("artifact dir");
            let record = usai_runtime::signing::sign_artifact(dir, &key)?;
            Some((record.files.len(), record.public_key))
        }
        None => None,
    };
    let m = out.definition.manifest();
    println!(
        "built {} ({} workloads, {} resources) in {:?}\n  {}\n  {}",
        out.definition.name(),
        m.workloads.len(),
        m.resources.len(),
        started.elapsed(),
        out.manifest_path.display(),
        out.code_path.display()
    );
    // An application with nothing to do builds, activates and serves 404s.
    // It is almost always a file the application never imports.
    if m.workloads.is_empty() {
        println!(
            "  warning: this application declares no workloads, so it will serve nothing.\n  \
             A workload reaches the application through an `import`: list it in \
             `defineApp({{ workloads: [...] }})` or in a module's `workloads`, \
             and pass the module in `defineApp({{ modules: [...] }})`. The runtime \
             never scans your files (docs/GUIDE.md §3)."
        );
    }
    let image = out
        .manifest_path
        .with_file_name("cache")
        .join("image.cwasm");
    if image.exists() {
        println!(
            "  {}  (engine cache for this host; not part of the artifact's identity)",
            image.display()
        );
    }
    if let Some((files, public)) = signed {
        println!(
            "  signed {files} files with key {}… (signature.json)",
            &public[..16]
        );
    }
    Ok(())
}

async fn definition_for(
    root: &Path,
    artifact: Option<PathBuf>,
) -> Result<(
    Arc<ApplicationDefinition>,
    Arc<dyn usai_runtime::engine::Engine>,
)> {
    definition_for_with(root, artifact, RuntimeConfig::default().max_worlds).await
}

async fn definition_for_with(
    root: &Path,
    artifact: Option<PathBuf>,
    capacity: u32,
) -> Result<(
    Arc<ApplicationDefinition>,
    Arc<dyn usai_runtime::engine::Engine>,
)> {
    let engine = engine_with(capacity);
    // An explicit --artifact is served as is — no project, no config, no
    // toolchain needed (that is how a production image runs). The project's
    // own build directory is reused only while it is newer than every source
    // it was built from, so inspect/graph/openapi/run never describe stale
    // code.
    if let Some(dir) = artifact {
        let definition = load_artifact(&dir)
            .await
            .with_context(|| format!("artifact {}", dir.display()))?;
        return Ok((definition, engine));
    }
    let config = load_config(engine.as_ref(), root).await?;
    let dir = config.out_dir.value.clone();
    let definition =
        if dir.join("manifest.json").exists() && usai_runtime::build::artifact_is_current(&dir) {
            load_artifact(&dir).await?
        } else {
            usai_runtime::build::build(engine.as_ref(), &BuildOptions::from_config(&config))
                .await?
                .definition
        };
    Ok((definition, engine))
}

#[allow(clippy::too_many_arguments)]
pub async fn run(
    root: &Path,
    host: &str,
    port: u16,
    artifact: Option<PathBuf>,
    status: bool,
    status_addr: Option<String>,
    control: Option<String>,
    announce: bool,
    require_signature: Vec<String>,
    max_worlds: u32,
    no_cron: bool,
    no_queue: bool,
    no_services: bool,
    drain_timeout: u64,
    drain_grace: u64,
    diagnostics: bool,
    server_timing: bool,
) -> Result<()> {
    // Take the port first. Loading and compiling an application is seconds of
    // work, and finding out afterwards that the address was already in use is
    // seconds spent for nothing.
    let listener = bind_early(host, port).await?;
    let trusted = trusted_signers(&require_signature)?;
    if !trusted.is_empty() {
        let dir = match &artifact {
            Some(dir) => dir.clone(),
            None => anyhow::bail!(
                "--require-signature needs --artifact <dir>: only a built, signed artifact can be verified"
            ),
        };
        let record = usai_runtime::signing::verify_artifact(&dir, &trusted)
            .map_err(|e| anyhow::anyhow!("artifact refused: {e}"))?;
        tracing::info!(
            artifact = %dir.display(),
            key = &record.public_key[..16],
            files = record.files.len(),
            "artifact signature verified"
        );
    }
    let (definition, engine) = definition_for_with(root, artifact, max_worlds.max(1)).await?;
    let runtime = Runtime::new(
        engine,
        RuntimeConfig {
            trusted_signers: trusted,
            max_worlds: max_worlds.max(1),
            default_app_concurrency: max_worlds.max(1),
            cron_scheduler: !no_cron,
            queue_consumers: !no_queue,
            services: !no_services,
            drain_timeout: Duration::from_secs(drain_timeout.max(1)),
            ..RuntimeConfig::default()
        },
    );
    // A preflight the operator cannot do from outside: only the process knows
    // both its cgroup limit and how many worlds it was told it may create.
    // The failure this prevents is not a crash — a container at its ceiling
    // reclaims the pages it executes from and slows to a crawl without being
    // killed or logging anything (docs/runbooks/memory-pressure.md).
    if let Some(p) = usai_runtime::procfs::read()
        && p.memory_limit_bytes > 0
    {
        let limit_mib = p.memory_limit_bytes / (1024 * 1024);
        // A **floor**, not a forecast. Sizing an instance properly is the
        // runbook's arithmetic (40 MiB + peak concurrency × 4…8 + 30 for a
        // held revision) and needs the application's own peak concurrency,
        // which this process cannot know at start. What it can know is the
        // point below which the box cannot hold the runtime, one held
        // revision and the *unique* memory of its slots at all — ≈1 MiB per
        // slot, measured (P8E: ≈1.5 MiB PSS per touched slot, the rest is
        // image pages every slot shares).
        //
        // The first version of this check used the full 4 MiB per slot and
        // fired at the envelope `SUPPORTED.md` tells operators to deploy on
        // (192 MiB at 48 worlds), which trains people to ignore it — and the
        // failure it guards against is the one Kubernetes cannot see. The
        // cells behind the threshold: 128 MiB / 48 worlds runs the production
        // shape with **zero** reclaim events, 64 MiB / 16 worlds runs hello
        // with 94, and 48 MiB / 16 worlds thrashes at 1.4 million
        // (`docs/measurements/2026-09-23-floor-accounting.md`).
        let floor_mib = 40 + 30 + u64::from(max_worlds.max(1));
        if limit_mib < floor_mib {
            tracing::warn!(
                limit_mib,
                max_worlds = max_worlds.max(1),
                floor_mib,
                "the memory limit is below the floor for this --max-worlds: at the limit the process reclaims the pages it executes from instead of failing, so it slows to a crawl without being killed or logging anything. Raise the limit or lower --max-worlds (docs/runbooks/sizing.md); watch usai_process_memory_ceiling_hits_total"
            );
        }
    }
    if no_cron {
        tracing::info!("cron scheduler off on this instance (--no-cron)");
    }
    if no_queue {
        tracing::info!("queue consumers off on this instance (--no-queue)");
    }
    if no_services {
        tracing::info!("services off on this instance (--no-services)");
    }
    // `USAI_ACTIVATION_RETRY=<seconds>`: keep trying to open the resources
    // for this long before giving up. Unset (the default) is exactly today's
    // behaviour — activation fails and the process exits, which is right for
    // a missing variable and right on a VM.
    //
    // On an orchestrator it is not: an unreachable database at start means
    // the container exits, and a pod that restarts for an unrelated reason
    // during a blip (a node drain, a scale-up, a spot reclaim) then stays
    // down until the restart backoff expires — after the database is
    // healthy. With this set, the process stays alive and keeps trying, so a
    // startup probe with enough budget covers the outage and nothing
    // restarts. Only a *dependency* is retried; a missing or malformed
    // variable is a configuration error and fails immediately, as before.
    // `docs/OPEN-QUESTIONS.md` → Q20 is whether this should be the default.
    let retry_for = std::env::var("USAI_ACTIVATION_RETRY")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .map(Duration::from_secs)
        .unwrap_or_default();
    let give_up_at = std::time::Instant::now() + retry_for;
    let _revision = loop {
        // A failed attempt must not leave its revision behind: they are
        // bounded (8), and a minute of retries would otherwise hit the bound
        // and turn a dependency problem into "too many revisions are held".
        let installed = runtime.install(Arc::clone(&definition)).await;
        let attempt = match installed {
            Ok(revision) => {
                let id = revision.id;
                match runtime.activate(id).await {
                    Ok(revision) => Ok(revision),
                    Err(e) => {
                        let _ = runtime.remove(id);
                        Err(e)
                    }
                }
            }
            Err(e) => Err(e),
        };
        match attempt {
            Ok(revision) => break revision,
            Err(e) => {
                let dependency = matches!(
                    e,
                    usai_runtime::RuntimeError::Resource(
                        usai_runtime::resource::ResourceError::Startup(..)
                    )
                );
                if dependency && std::time::Instant::now() < give_up_at {
                    tracing::warn!(
                        error = %e,
                        retry_seconds = retry_for.as_secs(),
                        "activation failed on a dependency; staying up and retrying (USAI_ACTIVATION_RETRY)"
                    );
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    continue;
                }
                return Err(match e {
                    usai_runtime::RuntimeError::MissingEnv(name) => anyhow::anyhow!(
                        "missing required environment: {name}\n  `usai run` reads the process environment only — it does not load .env (that is a development convenience of `usai dev`). Export {name} (or pass it through your orchestrator / compose `environment:`) and start again."
                    ),
                    other => other.into(),
                });
            }
        }
    };
    let (control_tx, control_rx) = tokio::sync::oneshot::channel::<String>();
    let stop_requested = match control {
        Some(addr) => {
            let addr: std::net::SocketAddr = addr.parse().context("invalid --control address")?;
            let token = std::env::var("USAI_CONTROL_TOKEN")
                .ok()
                .filter(|t| !t.is_empty());
            let host = usai_runtime::control::ControlHost::new(
                Arc::clone(&runtime),
                usai_runtime::control::ControlConfig { addr, token },
            )?;
            let stop = host.stop_requested.clone();
            let shutdown = runtime.shutdown_token();
            tokio::spawn(async move {
                let mut tx = Some(control_tx);
                if let Err(e) = usai_runtime::control::serve(host, shutdown, |bound| {
                    if let Some(tx) = tx.take() {
                        let _ = tx.send(format!("http://{bound}"));
                    }
                    if !announce && !machine_output() {
                        eprintln!("Control   http://{bound}");
                    }
                })
                .await
                {
                    eprintln!("control surface failed: {e}");
                }
            });
            Some(stop)
        }
        None => {
            drop(control_tx);
            None
        }
    };
    // `--announce`: one JSON line on stdout with the bound addresses, for
    // harnesses that spawn the runtime (usai/test).
    let on_ready: Option<Box<dyn FnOnce(String) + Send>> = if announce {
        let has_control = stop_requested.is_some();
        Some(Box::new(move |url: String| {
            tokio::spawn(async move {
                let control = if has_control {
                    control_rx.await.ok()
                } else {
                    None
                };
                println!("{}", serde_json::json!({ "app": url, "control": control }));
            });
        }))
    } else {
        None
    };
    serve_until_signal(
        runtime,
        listener,
        host,
        port,
        diagnostics,
        server_timing,
        status,
        status_addr,
        stop_requested,
        on_ready,
        Duration::from_secs(drain_grace),
    )
    .await
}

pub async fn graph(root: &Path) -> Result<()> {
    let (definition, _) = definition_for(root, None).await?;
    print!("{}", usai_runtime::observability::render_graph(&definition));
    Ok(())
}

/// `--log-format json`: no human banner on stdout, so everything the process
/// writes is one JSON object per line on stderr.
static MACHINE_OUTPUT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn suppress_human_output() {
    MACHINE_OUTPUT.store(true, std::sync::atomic::Ordering::SeqCst);
}

fn machine_output() -> bool {
    MACHINE_OUTPUT.load(std::sync::atomic::Ordering::SeqCst)
}

#[allow(clippy::too_many_arguments)]
/// Binds the application's port before the work of loading an application,
/// so `Address already in use` arrives in milliseconds. `--port 0` (tests,
/// harnesses) keeps its meaning: the bound port is the real one.
async fn bind_early(host: &str, port: u16) -> Result<Option<tokio::net::TcpListener>> {
    let Ok(addr) = format!("{host}:{port}").parse::<std::net::SocketAddr>() else {
        // An unparseable address is reported by the serve path, with context.
        return Ok(None);
    };
    match tokio::net::TcpListener::bind(addr).await {
        Ok(listener) => Ok(Some(listener)),
        Err(e) => Err(anyhow::anyhow!("cannot listen on {addr}: {e}")),
    }
}

#[allow(clippy::too_many_arguments)]
async fn serve_until_signal(
    runtime: Arc<Runtime>,
    listener: Option<tokio::net::TcpListener>,
    host: &str,
    port: u16,
    expose_diagnostics: bool,
    server_timing: bool,
    serve_status: bool,
    status_addr: Option<String>,
    stop_requested: Option<CancellationToken>,
    on_ready: Option<Box<dyn FnOnce(String) + Send>>,
    drain_grace: Duration,
) -> Result<()> {
    // Before anything can be served. `tokio::signal::unix::signal` installs
    // the handler when it is created; `ctrl_c()` installs it on first poll,
    // which used to be *after* the listener was already answering — a
    // SIGINT or SIGTERM in that window took the process's default action and
    // killed it without a drain. An orchestrator that starts a container and
    // changes its mind (a failed deploy, a fast rollback) lands exactly
    // there, and a loaded box widens the window.
    let mut interrupt = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())
        .context("cannot listen for SIGINT")?;
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .context("cannot listen for SIGTERM")?;
    let addr: std::net::SocketAddr = match &listener {
        // The port was taken before the application was loaded; `--port 0`
        // means the bound one is the only true address.
        Some(l) => l
            .local_addr()
            .context("the bound listener has no address")?,
        None => format!("{host}:{port}")
            .parse()
            .context("invalid listen address")?,
    };
    let http = HttpHost::new(
        Arc::clone(&runtime),
        HttpConfig {
            addr,
            expose_diagnostics,
            server_timing,
            // The reference is a runtime-owned surface like status and
            // metrics: on in dev, and wherever --status is asked for.
            serve_docs: expose_diagnostics || serve_status,
            serve_status,
            // A WebSocket that sends nothing for this long is closed (1008);
            // the reliability campaign shortens it to exercise the path.
            socket_idle_timeout: std::env::var("USAI_SOCKET_IDLE_TIMEOUT")
                .ok()
                .and_then(|v| v.parse::<u64>().ok())
                .map(Duration::from_secs)
                .unwrap_or(HttpConfig::default().socket_idle_timeout),
            // The request body bound, before any world exists (413 above it).
            // 1 MiB by default; a service that takes uploads on a raw route
            // raises it (USAI_MAX_BODY_BYTES, bytes).
            max_body_bytes: std::env::var("USAI_MAX_BODY_BYTES")
                .ok()
                .and_then(|v| v.parse::<usize>().ok())
                .filter(|n| *n > 0)
                .unwrap_or(HttpConfig::default().max_body_bytes),
            // Operator surfaces (status, metrics, docs) take a bearer token
            // when one is configured; the probes stay open.
            status_token: std::env::var("USAI_STATUS_TOKEN")
                .ok()
                .filter(|t| !t.is_empty()),
            surfaces_off: std::env::var("USAI_SURFACES_OFF")
                .ok()
                .map(|v| {
                    v.split(',')
                        .map(|s| s.trim().to_ascii_lowercase())
                        .filter(|s| !s.is_empty())
                        .collect()
                })
                .unwrap_or_default(),
            // On by default: a replica that cannot reach a bound resource
            // asks to be taken out of rotation. Off (`0`, `false`, `no`,
            // `off`) keeps resource-free routes serving through a shared
            // outage instead — the trade-off is in
            // docs/runbooks/postgres-down.md.
            ready_requires_resources: std::env::var("USAI_READY_REQUIRES_RESOURCES")
                .map(|v| {
                    !matches!(
                        v.trim().to_ascii_lowercase().as_str(),
                        "" | "0" | "false" | "no" | "off"
                    )
                })
                .unwrap_or(true),
        },
    );
    // A deployment-wide policy belongs in the log that records the start:
    // an operator reading why a replica stayed in rotation through an outage
    // should find the reason here rather than in someone's environment file.
    if !http.config().ready_requires_resources {
        tracing::info!(
            "readiness ignores resource health: /_usai/ready answers 200 while a bound resource fails its probe, and names it in the body (USAI_READY_REQUIRES_RESOURCES=0)"
        );
    }
    // A harness (`--announce`) wants a silent exit; `dev` narrates like `run`.
    let quiet = on_ready.is_some() && !expose_diagnostics;
    let shutdown = CancellationToken::new();
    let http_for_drain = Arc::clone(&http);
    // The private surfaces on their own listener, when asked.
    let http_for_internal = Arc::clone(&http);
    if let Some(addr) = status_addr {
        let addr: std::net::SocketAddr = addr.parse().context("invalid --status-addr")?;
        let token = shutdown.clone();
        // What this listener will actually answer, not what it could: a
        // surface removed with USAI_SURFACES_OFF is not announced.
        let surfaces = http_for_internal.internal_surfaces().join(", ");
        tokio::spawn(async move {
            if let Err(e) =
                usai_runtime::http::serve_internal(http_for_internal, addr, token, |bound| {
                    tracing::info!(%bound, "status listener: {surfaces}");
                })
                .await
            {
                tracing::error!(error = %e, "status listener failed");
            }
        });
    }
    // The routing table before the first request, not during it.
    http_for_drain.warm();
    let server = {
        let shutdown = shutdown.clone();
        let (tx, rx) = tokio::sync::oneshot::channel();
        let handle = tokio::spawn(async move {
            match listener {
                Some(l) => {
                    usai_runtime::http::serve_on(l, http, shutdown, |addr| {
                        let _ = tx.send(addr);
                    })
                    .await
                }
                None => {
                    serve(http, shutdown, |addr| {
                        let _ = tx.send(addr);
                    })
                    .await
                }
            }
        });
        let bound = match rx.await {
            Ok(bound) => bound,
            Err(_) => {
                // The server task returned before announcing: its error is
                // the real reason (an address already in use, most often).
                return match handle.await {
                    Ok(Err(e)) => Err(e).with_context(|| format!("cannot listen on {addr}")),
                    Ok(Ok(())) => anyhow::bail!("server exited before binding {addr}"),
                    Err(e) => Err(e).context("server task failed"),
                };
            }
        };
        let url = format!("http://{bound}");
        match on_ready {
            Some(f) => f(url),
            None if machine_output() => {
                // The same facts the banner carries, as one log line.
                let revision = runtime.active()?;
                let m = revision.definition.manifest();
                tracing::info!(
                    application = revision.definition.name(),
                    revision = %revision.id,
                    identity = revision.definition.identity(),
                    url = %url,
                    workloads = m.workloads.len(),
                    resources = m.resources.len(),
                    "serving"
                );
            }
            None => {
                let revision = runtime.active()?;
                print!(
                    "{}",
                    display::banner(
                        &revision.definition,
                        &format!("{} ({})", revision.id, revision.definition.identity()),
                        Some(&url),
                        Some(&runtime.status()),
                        expose_diagnostics,
                        serve_status,
                        false,
                    )
                );
            }
        }
        handle
    };
    // SIGINT (a terminal) and SIGTERM (an orchestrator, `docker stop`) both
    // mean "drain, then leave"; a second one forces the exit. A wrapper that
    // started this process (`pnpm usai`, an IDE task) names itself in
    // USAI_PARENT_PID: when it is gone, so is the reason to keep serving —
    // otherwise a killed wrapper leaves a server holding the port.
    let parent_gone = async {
        match std::env::var("USAI_PARENT_PID")
            .ok()
            .and_then(|p| p.parse::<i32>().ok())
        {
            Some(pid) => loop {
                tokio::time::sleep(Duration::from_secs(1)).await;
                // Signal 0 checks existence without delivering anything.
                if unsafe { libc::kill(pid, 0) } != 0 {
                    break;
                }
            },
            None => std::future::pending().await,
        }
    };
    tokio::select! {
        _ = interrupt.recv() => {}
        _ = terminate.recv() => { if !quiet { tracing::info!("SIGTERM received"); } }
        _ = parent_gone => { tracing::info!("the process that started usai is gone; shutting down"); }
        _ = async { match &stop_requested { Some(t) => t.cancelled().await, None => std::future::pending().await } } => {
            if !quiet { tracing::info!("stop requested through the control surface"); }
        }
    }
    // Under a harness or orchestrator (`--announce`) the shutdown narration
    // is noise in someone else's output; the exit code carries the result.
    // First the balancer learns: readiness fails and responses close their
    // connection while the listener still serves, for the grace period. A
    // proxy that reused an idle keep-alive connection at the instant the
    // listener closed used to see EOF — a 502 its client never asked for.
    if !drain_grace.is_zero() {
        http_for_drain.begin_draining();
        // The grace is for the balancer and for requests already in flight.
        // Background work has no balancer watching it, so it stops now
        // rather than claiming more work this instance then has to finish
        // while it is leaving.
        runtime.stop_background_work();
        if !quiet {
            tracing::info!(
                grace_seconds = drain_grace.as_secs(),
                "shutting down: readiness now fails, cron and queue consumers stop claiming, and connections close after their response; the listener closes after the grace period"
            );
        }
        tokio::select! {
            _ = tokio::time::sleep(drain_grace) => {}
            _ = async { tokio::select! { _ = interrupt.recv() => {}, _ = terminate.recv() => {} } } => {
                if !quiet { tracing::info!("second signal: closing the listener now"); }
            }
        }
    }
    if !quiet {
        tracing::info!("shutting down: draining in-flight work (a second signal forces the exit)");
    }
    shutdown.cancel();
    // The server's own connections get the runtime's drain bound plus a
    // margin, so the total stays predictable for the orchestrator's grace
    // period (`--drain-timeout`).
    let connection_bound = runtime.config().drain_timeout + Duration::from_secs(5);
    let drain = async {
        runtime.shutdown().await;
        let _ = tokio::time::timeout(connection_bound, server).await;
    };
    tokio::select! {
        _ = drain => { if !quiet { tracing::info!("drained; ownership returned to baseline"); } }
        _ = async { tokio::select! { _ = interrupt.recv() => {}, _ = terminate.recv() => {} } } => {
            let g = runtime.ledger().gauges.snapshot();
            tracing::warn!(live_worlds = g.live_worlds, live_ops = g.live_ops, "forced shutdown");
            std::process::exit(130);
        }
    }
    Ok(())
}

pub async fn inspect(root: &Path, json: bool) -> Result<()> {
    let (definition, _) = definition_for(root, None).await?;
    if json {
        println!("{}", serde_json::to_string_pretty(definition.manifest())?);
    } else {
        print!(
            "{}",
            display::inspect(
                &definition,
                usai_runtime::RuntimeConfig::default()
                    .default_timeout
                    .as_millis() as u64
            )
        );
    }
    Ok(())
}

pub async fn config(root: &Path, json: bool) -> Result<()> {
    let engine = engine();
    let config = load_config(engine.as_ref(), root).await?;
    if json {
        println!("{}", serde_json::to_string_pretty(&config)?);
        return Ok(());
    }
    let rel = |p: &Path| {
        p.strip_prefix(&config.root)
            .map(|p| format!("./{}", p.display()))
            .unwrap_or_else(|_| p.display().to_string())
    };
    println!("Project root\n  {}\n", config.root.display());
    println!(
        "Config file\n  {}\n",
        config
            .config_file
            .as_deref()
            .map(rel)
            .unwrap_or_else(|| "none (usai.config.ts not found)".into())
    );
    println!(
        "Application entry\n  {}\n  source: {}\n",
        rel(&config.app.value),
        config.app.source
    );
    println!(
        "Build output\n  {}\n  source: {}\n",
        rel(&config.out_dir.value),
        config.out_dir.source
    );
    println!(
        "Migrations\n  {}\n  source: {}\n",
        config.migrations.value.join("\n  "),
        config.migrations.source
    );
    println!(
        "Seeders\n  {}\n  source: {}",
        config.seeders.value.join("\n  "),
        config.seeders.source
    );
    Ok(())
}

/// `usai dev`: rapid revision replacement (ADR-0006). Every change builds a
/// new definition; only a successful build becomes the active revision, and
/// the previous one drains.
pub async fn dev(root: &Path, host: &str, port: u16) -> Result<()> {
    // Before the first build: a taken port should not cost a compile.
    let listener = bind_early(host, port).await?;
    let engine = engine();
    let config = load_config(engine.as_ref(), root).await?;
    let options = BuildOptions::from_config(&config);
    let first = usai_runtime::build::build(engine.as_ref(), &options).await?;
    // `.env` is read at start and again on every rebuild, never overriding
    // what the shell set: editing it and saving a source file is enough.
    let dotenv_path = root.join(".env");
    let dotenv: Arc<std::sync::RwLock<std::collections::HashMap<String, String>>> = Arc::new(
        std::sync::RwLock::new(crate::parse_dotenv(&dotenv_path).into_iter().collect()),
    );
    {
        let n = dotenv.read().expect("dotenv poisoned").len();
        if n > 0 {
            eprintln!("loaded {n} variable(s) from .env");
        }
    }
    let env_source = Arc::clone(&dotenv);
    let runtime = Runtime::with_env(Arc::clone(&engine), RuntimeConfig::default(), move |name| {
        std::env::var(name).ok().or_else(|| {
            env_source
                .read()
                .expect("dotenv poisoned")
                .get(name)
                .cloned()
        })
    });
    let revision = runtime.install(first.definition).await?;
    // A failed first activation (typically the environment) is not the end
    // of the session: nothing is served (503 `no_active_revision`) and the
    // watcher stays up, so fixing `.env` or the source and saving recovers.
    if let Err(e) = runtime.activate(revision.id).await {
        let hint = match &e {
            usai_runtime::RuntimeError::MissingEnv(name) => format!(
                "
  `usai dev` reads the shell environment and {}; add {name}=… there and save — the revision activates on the next change.",
                dotenv_path.display()
            ),
            _ => String::new(),
        };
        eprintln!(
            "activation failed: {e:#}{hint}
(nothing is served until a revision activates; watching for changes)"
        );
    }

    let watched: Arc<std::sync::Mutex<Vec<PathBuf>>> =
        Arc::new(std::sync::Mutex::new(first.inputs));
    let (tx, mut rx) = tokio::sync::mpsc::channel::<()>(8);
    let watch_root = config
        .app
        .value
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| root.to_path_buf());
    let watcher_paths = Arc::clone(&watched);
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        if let Ok(event) = event {
            // Only content changes. inotify also reports reads (Access)
            // and metadata (Other): the build reading its own inputs would
            // otherwise re-trigger itself forever.
            if !matches!(
                event.kind,
                notify::EventKind::Create(_)
                    | notify::EventKind::Modify(_)
                    | notify::EventKind::Remove(_)
            ) {
                return;
            }
            if event.paths.iter().any(|p| {
                p.components()
                    .any(|c| c.as_os_str() == ".usai" || c.as_os_str() == "node_modules")
            }) {
                return;
            }
            let relevant = {
                let paths = watcher_paths.lock().expect("watched poisoned");
                event.paths.iter().any(|p| {
                    paths.iter().any(|w| w == p)
                        || p.file_name().is_some_and(|f| f == ".env")
                        || p.extension()
                            .is_some_and(|e| e == "ts" || e == "js" || e == "json")
                })
            };
            if relevant {
                let _ = tx.try_send(());
            }
        }
    })?;
    use notify::Watcher as _;
    watcher.watch(&watch_root, notify::RecursiveMode::Recursive)?;
    // `.env` lives at the project root, which the application directory
    // need not contain: watch the root's own entries too (non-recursively),
    // so creating or editing `.env` re-activates without a source change.
    if watch_root != root {
        watcher.watch(root, notify::RecursiveMode::NonRecursive)?;
    }
    if let Some(config_file) = &config.config_file {
        watcher.watch(config_file, notify::RecursiveMode::NonRecursive)?;
    }

    let rebuild_runtime = Arc::clone(&runtime);
    let rebuild_engine = Arc::clone(&engine);
    let rebuild_root = root.to_path_buf();
    let rebuild_dotenv = Arc::clone(&dotenv);
    tokio::spawn(async move {
        while rx.recv().await.is_some() {
            // Coalesce bursts of filesystem events.
            tokio::time::sleep(Duration::from_millis(120)).await;
            while rx.try_recv().is_ok() {}
            let started = std::time::Instant::now();
            let env_changed = {
                let fresh: std::collections::HashMap<String, String> =
                    crate::parse_dotenv(&rebuild_root.join(".env"))
                        .into_iter()
                        .collect();
                let mut current = rebuild_dotenv.write().expect("dotenv poisoned");
                if *current != fresh {
                    eprintln!("\n.env changed: {} variable(s) now loaded", fresh.len());
                    *current = fresh;
                    true
                } else {
                    false
                }
            };
            let config = match load_config(rebuild_engine.as_ref(), &rebuild_root).await {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("\nconfig error: {e:#}\n(previous revision keeps serving)");
                    continue;
                }
            };
            match usai_runtime::build::build(
                rebuild_engine.as_ref(),
                &BuildOptions::from_config(&config),
            )
            .await
            {
                Ok(out) => {
                    *watched.lock().expect("watched poisoned") = out.inputs;
                    // Types are checked in the background: the new revision
                    // serves meanwhile, the diagnostics arrive when tsc is done.
                    let check_root = rebuild_root.clone();
                    tokio::spawn(async move {
                        if let Some(Err(diagnostics)) = typecheck(&check_root).await {
                            eprintln!(
                                "\ntype errors (the revision serves anyway; `usai build` refuses them):\n{diagnostics}"
                            );
                        }
                    });
                    let previous = rebuild_runtime.active().ok();
                    // The environment is bound at activation, not part of the
                    // identity: a changed .env needs a new revision even when
                    // the application is byte-identical.
                    if !env_changed
                        && previous
                            .as_ref()
                            .is_some_and(|p| p.definition.identity() == out.definition.identity())
                    {
                        eprintln!(
                            "\nrebuilt in {:?}: the application is unchanged ({}); keeping revision {}",
                            started.elapsed(),
                            out.definition.identity(),
                            previous
                                .as_ref()
                                .map(|p| p.id.to_string())
                                .unwrap_or_default()
                        );
                        continue;
                    }
                    match rebuild_runtime.install(out.definition).await {
                        Ok(rev) => match rebuild_runtime.activate(rev.id).await {
                            Ok(rev) => {
                                eprintln!(
                                    "\nrevision {} active ({}) in {:?}{}",
                                    rev.id,
                                    rev.definition.identity(),
                                    started.elapsed(),
                                    display::workload_diff(
                                        previous.as_ref().map(|p| p.definition.as_ref()),
                                        &rev.definition
                                    )
                                );
                                if let Some(prev) = previous {
                                    let rt = Arc::clone(&rebuild_runtime);
                                    tokio::spawn(async move {
                                        // The replaced revision retires itself once settled; an
                                        // explicit drain only hurries it, so "unknown revision" here
                                        // means it already left.
                                        if let Err(e) = rt.drain(prev.id).await
                                            && !matches!(
                                                e,
                                                usai_runtime::RuntimeError::UnknownRevision(_)
                                            )
                                        {
                                            eprintln!("drain of {} failed: {e}", prev.id);
                                        }
                                    });
                                }
                            }
                            Err(e) => {
                                let serving = if rebuild_runtime.active().is_ok() {
                                    "previous revision keeps serving"
                                } else {
                                    "nothing is served until a revision activates"
                                };
                                let hint = match &e {
                                    usai_runtime::RuntimeError::MissingEnv(name) => {
                                        format!("\n  add {name}=… to .env (or export it) and save")
                                    }
                                    _ => String::new(),
                                };
                                eprintln!("\nactivation failed: {e:#}{hint}\n({serving})");
                            }
                        },
                        Err(e) => {
                            eprintln!("\ninstall failed: {e:#}\n(previous revision keeps serving)")
                        }
                    }
                }
                Err(e) => eprintln!("\nbuild failed: {e:#}\n(previous revision keeps serving)"),
            }
        }
    });

    let banner_runtime = Arc::clone(&runtime);
    serve_until_signal(
        runtime,
        listener,
        host,
        port,
        true,
        true,
        true,
        None,
        None,
        Some(Box::new(move |url| {
            match banner_runtime.active() {
                Ok(revision) => print!(
                    "{}",
                    display::banner(
                        &revision.definition,
                        &format!("{} ({})", revision.id, revision.definition.identity()),
                        Some(&url),
                        Some(&banner_runtime.status()),
                        true,
                        true,
                        true,
                    )
                ),
                Err(_) => println!("listening on {url} — no active revision yet (requests answer 503 no_active_revision)"),
            }
            println!("\nwatching for changes (ctrl-c to stop)");
        })),
        Duration::ZERO,
    )
    .await
}

/// Runs one finite world against the built artifact and reports its
/// outcome. Shared by `usai app`, `usai cron run`, and `usai task run`.
async fn one_shot(
    root: &Path,
    run: impl AsyncFnOnce(&Runtime) -> Result<WorkResult, RuntimeError>,
) -> Result<()> {
    let (definition, engine) = definition_for(root, None).await?;
    let runtime = Runtime::new(
        engine,
        RuntimeConfig {
            cron_scheduler: false,
            queue_consumers: false,
            services: false,
            ..RuntimeConfig::default()
        },
    );
    let revision = runtime.install(definition).await?;
    runtime.activate(revision.id).await?;
    let result = run(&runtime).await;
    runtime.shutdown().await;
    // "unknown workload command:nope" is not a definition problem: name what
    // exists of that kind so the typo is obvious.
    let result = match result {
        Err(RuntimeError::UnknownWorkload(id)) => {
            let (kind, name) = id.split_once(':').unwrap_or(("", id.as_str()));
            let mut available: Vec<&str> = revision
                .definition
                .manifest()
                .workloads
                .iter()
                .filter(|w| w.id.starts_with(&format!("{kind}:")))
                .map(|w| w.name.as_str())
                .collect();
            available.sort();
            anyhow::bail!(
                "no {kind} named {name:?}; declared {kind}s: {}",
                if available.is_empty() {
                    "(none)".to_owned()
                } else {
                    available.join(", ")
                }
            );
        }
        other => other?,
    };
    // The application's lines were already streamed live by the `app`
    // tracing target (with the workload and world); nothing is echoed twice.
    for violation in &result.violations {
        eprintln!("\nlifecycle: {}\n", violation.message);
    }
    match (&result.termination, &result.outcome) {
        (Termination::Completed, Some(Ok(value))) => {
            let value = value.get("value").unwrap_or(value);
            println!("{}", serde_json::to_string_pretty(value)?);
            Ok(())
        }
        (Termination::Completed, Some(Err(error))) => {
            anyhow::bail!(
                "{}: {}{}",
                error.name,
                error.message,
                error
                    .usai
                    .as_ref()
                    .map(|u| format!(" ({u})"))
                    .unwrap_or_default()
            )
        }
        (termination, _) => anyhow::bail!("work ended without a result: {termination:?}"),
    }
}

pub async fn app(root: &Path, name: &str, args: Vec<String>) -> Result<()> {
    one_shot(root, async |rt| rt.run_command(name, args).await).await
}

pub async fn cron_run(root: &Path, name: &str) -> Result<()> {
    one_shot(root, async |rt| rt.run_cron(name).await).await
}

pub async fn task_run(root: &Path, name: &str, input: &str) -> Result<()> {
    let input: serde_json::Value = serde_json::from_str(input).context("--input must be JSON")?;
    one_shot(root, async |rt| rt.run_task(name, input).await).await
}

/// `usai probe live|ready`: one GET against the status listener, exit 0 on
/// 200. Made for `HEALTHCHECK` / `livenessProbe` in an image that carries
/// no curl; the body is printed when the answer is not 200 so a failing
/// readiness names its resource.
pub async fn probe(which: &str, addr: &str, timeout: u64) -> Result<()> {
    if which != "live" && which != "ready" {
        anyhow::bail!("usai probe <live|ready>");
    }
    let url = format!("http://{addr}/_usai/{which}");
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(timeout.max(1)))
        .build()?;
    let response = client
        .get(&url)
        .send()
        .await
        .with_context(|| format!("{url}: no answer"))?;
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    if status.is_success() {
        println!("{which}: ok");
        Ok(())
    } else {
        eprintln!("{which}: {status} {body}");
        std::process::exit(1)
    }
}

pub async fn queue_run(root: &Path, topic: &str, message: &str) -> Result<()> {
    let message: serde_json::Value =
        serde_json::from_str(message).context("--message must be JSON")?;
    one_shot(root, async |rt| rt.run_queue_message(topic, message).await).await
}

/// `usai queue status`: what is in the table, per topic and state.
pub async fn queue_status(
    root: &Path,
    topic: Option<&str>,
    resource: Option<&str>,
    json: bool,
) -> Result<()> {
    with_active_runtime(root, async |runtime, _config| {
        let revision = runtime.active()?;
        let manager = db::database(&revision, resource)?;
        let rows = match usai_runtime::workloads::queue::ops::stats(manager.as_ref()).await {
            Ok(rows) => rows,
            // The table is created by the first publish or the first
            // consumer; before that there is nothing to report, which is an
            // answer rather than an error.
            Err(e) if e.to_string().contains("42p01") => {
                println!(
                    "usai_queue does not exist in this database yet: nothing has been published and no consumer has started."
                );
                return Ok(());
            }
            Err(e) => return Err(e.into()),
        };
        let rows: Vec<serde_json::Value> = rows
            .as_array()
            .map(|r| {
                r.iter()
                    .filter(|row| {
                        topic.is_none_or(|t| row.get("topic").and_then(|v| v.as_str()) == Some(t))
                    })
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        if json {
            println!("{}", serde_json::to_string_pretty(&rows)?);
            return Ok(());
        }
        if rows.is_empty() {
            println!("usai_queue is empty (or does not exist yet)");
            return Ok(());
        }
        println!(
            "{:<28} {:<11} {:>10} {:>14} {:>14}",
            "topic", "state", "rows", "oldest wait", "oldest claim"
        );
        let secs = |row: &serde_json::Value, key: &str| match row.get(key).and_then(|v| v.as_i64()) {
            Some(s) if s >= 86_400 => format!("{}d", s / 86_400),
            Some(s) if s >= 3_600 => format!("{}h", s / 3_600),
            Some(s) if s >= 60 => format!("{}m", s / 60),
            Some(s) => format!("{s}s"),
            None => "-".to_owned(),
        };
        for row in &rows {
            println!(
                "{:<28} {:<11} {:>10} {:>14} {:>14}",
                row.get("topic").and_then(|v| v.as_str()).unwrap_or("?"),
                row.get("state").and_then(|v| v.as_str()).unwrap_or("?"),
                row.get("rows").and_then(|v| v.as_i64()).unwrap_or(0),
                secs(row, "oldest_wait_seconds"),
                secs(row, "oldest_claim_seconds"),
            );
        }
        println!(
            "\n`ready` is depth; a `processing` row older than the consumer's deadline is a lost\nconsumer the sweeper will reclaim; `dead` is yours to inspect and to prune."
        );
        Ok(())
    })
    .await
}

/// `usai queue prune`: delete finished rows, on purpose and never by itself.
pub async fn queue_prune(
    root: &Path,
    state: &str,
    older_than: &str,
    topic: Option<&str>,
    resource: Option<&str>,
    dry_run: bool,
) -> Result<()> {
    let states: Vec<&str> = match state {
        "done" => vec!["done"],
        "dead" => vec!["dead"],
        "all" => vec!["done", "dead"],
        other => anyhow::bail!("--state is done, dead or all (got {other})"),
    };
    let seconds = parse_duration_seconds(older_than)?;
    with_active_runtime(root, async |runtime, _config| {
        let revision = runtime.active()?;
        let manager = db::database(&revision, resource)?;
        if dry_run {
            let n = usai_runtime::workloads::queue::ops::prune_count(
                manager.as_ref(),
                &states,
                seconds,
                topic,
            )
            .await?;
            println!(
                "dry run: {n} row(s) in state {} older than {older_than} would be deleted\n  nothing was deleted — add --yes to delete",
                states.join("/")
            );
            return Ok(());
        }
        let deleted =
            usai_runtime::workloads::queue::ops::prune(manager.as_ref(), &states, seconds, topic)
                .await?;
        println!(
            "deleted {deleted} row(s) in state {} older than {older_than}{}",
            states.join("/"),
            topic.map(|t| format!(" on {t}")).unwrap_or_default()
        );
        Ok(())
    })
    .await
}

/// `usai queue prepare`: build the indexes without blocking writers.
pub async fn queue_prepare(root: &Path, resource: Option<&str>) -> Result<()> {
    with_active_runtime(root, async |runtime, _config| {
        let revision = runtime.active()?;
        let manager = db::database(&revision, resource)?;
        let ran = usai_runtime::workloads::queue::ops::prepare_indexes(manager.as_ref()).await?;
        if ran.is_empty() {
            println!("nothing to build");
        } else {
            println!("built {} index(es) without blocking writers:", ran.len());
            for statement in ran {
                println!("  {statement}");
            }
        }
        println!(
            "\nRun this before deploying a version that adds an index: the runtime's own\n`CREATE INDEX IF NOT EXISTS` would build it under a write-blocking lock at the\nfirst use of the new version."
        );
        Ok(())
    })
    .await
}

/// `7d`, `48h`, `30m`, `90s`, or plain seconds.
fn parse_duration_seconds(value: &str) -> Result<i64> {
    let value = value.trim();
    let (digits, factor) = match value.chars().last() {
        Some('d') => (&value[..value.len() - 1], 86_400),
        Some('h') => (&value[..value.len() - 1], 3_600),
        Some('m') => (&value[..value.len() - 1], 60),
        Some('s') => (&value[..value.len() - 1], 1),
        _ => (value, 1),
    };
    let n: i64 = digits
        .trim()
        .parse()
        .with_context(|| format!("not a duration: {value} (try 7d, 48h, 30m, 90s)"))?;
    anyhow::ensure!(n >= 0, "a duration cannot be negative");
    Ok(n * factor)
}

/// Builds, activates (binding resources), runs `f`, and shuts down.
async fn with_active_runtime<T>(
    root: &Path,
    f: impl AsyncFnOnce(&Runtime, &usai_runtime::build::ProjectConfig) -> Result<T>,
) -> Result<T> {
    let engine = engine();
    let config = load_config(engine.as_ref(), root).await?;
    let out =
        usai_runtime::build::build(engine.as_ref(), &BuildOptions::from_config(&config)).await?;
    let runtime = Runtime::new(
        engine,
        RuntimeConfig {
            cron_scheduler: false,
            queue_consumers: false,
            services: false,
            ..RuntimeConfig::default()
        },
    );
    let revision = runtime.install(out.definition).await?;
    runtime.activate(revision.id).await?;
    let result = f(&runtime, &config).await;
    runtime.shutdown().await;
    result
}

/// A runtime over an artifact's definition (no project, no source), for
/// `db migrate --artifact` / `db status --artifact` inside a production image.
async fn with_artifact_runtime<T>(
    artifact: &Path,
    f: impl AsyncFnOnce(&Runtime, Vec<db::MigrationFile>) -> Result<T>,
) -> Result<T> {
    let definition = load_artifact(artifact)
        .await
        .with_context(|| format!("artifact {}", artifact.display()))?;
    let files = db::artifact_migrations(artifact)?;
    let runtime = Runtime::new(
        engine(),
        RuntimeConfig {
            cron_scheduler: false,
            queue_consumers: false,
            services: false,
            ..RuntimeConfig::default()
        },
    );
    let revision = runtime.install(definition).await?;
    runtime.activate(revision.id).await?;
    let result = f(&runtime, files).await;
    runtime.shutdown().await;
    result
}

pub async fn db_migrate(
    root: &Path,
    resource: Option<&str>,
    artifact: Option<PathBuf>,
) -> Result<()> {
    if let Some(dir) = artifact {
        return with_artifact_runtime(&dir, async |runtime, files| {
            let revision = runtime.active()?;
            let manager = db::database(&revision, resource)?;
            let applied = db::migrate(manager.as_ref(), &files, CancellationToken::new()).await?;
            report_applied(&applied, files.len());
            Ok(())
        })
        .await;
    }
    with_active_runtime(root, async |runtime, config| {
        let revision = runtime.active()?;
        let globs = db::migration_globs(&revision.definition, &config.migrations.value);
        let files = db::discover_migrations(&config.root, &globs)?;
        let manager = db::database(&revision, resource)?;
        let applied = db::migrate(manager.as_ref(), &files, CancellationToken::new()).await?;
        report_applied(&applied, files.len());
        Ok(())
    })
    .await
}

fn report_applied(applied: &[String], total: usize) {
    if applied.is_empty() {
        println!("nothing to apply ({total} migrations already applied)");
    } else {
        for name in applied {
            println!("applied {name}");
        }
    }
}

fn print_migration_status(status: Vec<db::MigrationStatus>, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(&status)?);
        return Ok(());
    }
    println!("{:<40} {:<18} applied", "migration", "checksum");
    for s in status {
        let applied = s.applied_at.as_deref().unwrap_or("pending");
        let missing = if s.path.is_none() {
            "  (file missing)"
        } else {
            ""
        };
        println!("{:<40} {:<18} {applied}{missing}", s.name, s.checksum);
    }
    Ok(())
}

pub async fn db_status(
    root: &Path,
    resource: Option<&str>,
    json: bool,
    artifact: Option<PathBuf>,
) -> Result<()> {
    if let Some(dir) = artifact {
        return with_artifact_runtime(&dir, async |runtime, files| {
            let revision = runtime.active()?;
            let manager = db::database(&revision, resource)?;
            print_migration_status(db::status(manager.as_ref(), &files).await?, json)
        })
        .await;
    }
    with_active_runtime(root, async |runtime, config| {
        let revision = runtime.active()?;
        let globs = db::migration_globs(&revision.definition, &config.migrations.value);
        let files = db::discover_migrations(&config.root, &globs)?;
        let manager = db::database(&revision, resource)?;
        print_migration_status(db::status(manager.as_ref(), &files).await?, json)
    })
    .await
}

pub async fn db_seed(root: &Path, name: Option<&str>) -> Result<()> {
    let engine = engine();
    let config = load_config(engine.as_ref(), root).await?;
    let options = BuildOptions::from_config(&config);
    let app = usai_runtime::build::build(engine.as_ref(), &options).await?;
    let globs = db::seeder_globs(&app.definition, &config.seeders.value);
    let seeders = db::discover_seeders(&config.root, &globs)?;
    let selected: Vec<_> = seeders
        .iter()
        .filter(|s| name.is_none_or(|n| s.name == n))
        .collect();
    if selected.is_empty() {
        anyhow::bail!(
            "no seeder {}found; looked in:\n  {}",
            name.map(|n| format!("named {n} ")).unwrap_or_default(),
            globs
                .iter()
                .map(|(g, s)| format!("{g}  ({s})"))
                .collect::<Vec<_>>()
                .join("\n  ")
        );
    }
    for seeder in selected {
        let out = build_seeder(engine.as_ref(), &options, &seeder.path, &seeder.name).await?;
        let runtime = Runtime::new(
            Arc::clone(&engine),
            RuntimeConfig {
                cron_scheduler: false,
                queue_consumers: false,
                services: false,
                ..RuntimeConfig::default()
            },
        );
        let revision = runtime.install(out.definition).await?;
        runtime.activate(revision.id).await?;
        let result = runtime
            .run_command(&format!("seed:{}", seeder.name), vec![])
            .await;
        runtime.shutdown().await;
        let result = result?;
        match (&result.termination, &result.outcome) {
            (Termination::Completed, Some(Ok(value))) => {
                // The seeder's return value, like a command's, is the report.
                let value = value.get("value").unwrap_or(value);
                let report = match value {
                    serde_json::Value::Null => String::new(),
                    v => format!(" → {v}"),
                };
                println!(
                    "seeded {} ({}){report}",
                    seeder.name,
                    seeder
                        .path
                        .strip_prefix(&config.root)
                        .unwrap_or(&seeder.path)
                        .display()
                )
            }
            (Termination::Completed, Some(Err(e))) => {
                anyhow::bail!("seeder {} failed: {}: {}", seeder.name, e.name, e.message)
            }
            (t, _) => anyhow::bail!("seeder {} ended without a result: {t:?}", seeder.name),
        }
    }
    Ok(())
}

pub async fn generate_openapi(root: &Path, out: Option<PathBuf>, public: bool) -> Result<()> {
    let (definition, _) = definition_for(root, None).await?;
    let config = RuntimeConfig::default();
    let profile = if public {
        usai_runtime::openapi::Profile::Public
    } else {
        usai_runtime::openapi::Profile::Internal
    };
    let document = usai_runtime::openapi::generate_with(&definition, &config, profile);
    let text = serde_json::to_string_pretty(&document)?;
    match out {
        Some(path) => {
            tokio::fs::write(&path, text).await?;
            eprintln!("wrote {}", path.display());
        }
        None => println!("{text}"),
    }
    Ok(())
}

/// `usai bench`: an engineering load test against an in-process server.
/// Reports latency percentiles, throughput, RSS high-water, and whether
/// ownership returned to baseline. Not canonical evidence (AGENTS.md §2).
pub async fn bench(root: &Path, path: &str, concurrency: usize, duration: Duration) -> Result<()> {
    // Always a fresh build: a measurement of a stale artifact (older SDK,
    // older sources) is a footgun, and the build is cheap.
    let engine = engine();
    let config = load_config(engine.as_ref(), root).await?;
    let definition =
        usai_runtime::build::build(engine.as_ref(), &BuildOptions::from_config(&config))
            .await?
            .definition;
    let runtime = Runtime::new(
        engine,
        RuntimeConfig {
            cron_scheduler: false,
            queue_consumers: false,
            services: false,
            ..RuntimeConfig::default()
        },
    );
    let revision = runtime.install(definition).await?;
    runtime.activate(revision.id).await?;
    let http = HttpHost::new(
        Arc::clone(&runtime),
        HttpConfig {
            addr: ([127, 0, 0, 1], 0).into(),
            ..HttpConfig::default()
        },
    );
    let shutdown = CancellationToken::new();
    let (tx, rx) = tokio::sync::oneshot::channel();
    let token = shutdown.clone();
    let server = tokio::spawn(async move {
        serve(http, token, |addr| {
            let _ = tx.send(addr);
        })
        .await
    });
    let addr = rx.await.context("server did not bind")?;
    let url = format!("http://{addr}{path}");
    let client = reqwest::Client::builder()
        .pool_max_idle_per_host(concurrency)
        .build()?;
    // Warm-up: definition-lifetime work (routing, validators) happens once.
    for _ in 0..(concurrency.min(16)) {
        let _ = client.get(&url).send().await;
    }
    let started = std::time::Instant::now();
    let deadline = started + duration;
    let mut workers = Vec::new();
    // Fixed-size latency histogram (10 µs buckets to 1 s, then one overflow
    // bucket) so a long soak measures the runtime's memory, not the client's.
    const BUCKET_US: u64 = 10;
    const BUCKETS: usize = 100_000;
    // Sampled every 10 s: requests and p50 over that window, so a soak shows
    // drift rather than only an end-of-run summary.
    let progress = duration > Duration::from_secs(30);
    for _ in 0..concurrency {
        let client = client.clone();
        let url = url.clone();
        workers.push(tokio::spawn(async move {
            let mut hist = vec![0u64; BUCKETS + 1];
            let mut max_us = 0u64;
            let mut errors = 0u64;
            while std::time::Instant::now() < deadline {
                let t = std::time::Instant::now();
                match client.get(&url).send().await {
                    Ok(r) if r.status().is_success() => {
                        let _ = r.bytes().await;
                        let us = t.elapsed().as_micros() as u64;
                        max_us = max_us.max(us);
                        hist[((us / BUCKET_US) as usize).min(BUCKETS)] += 1;
                    }
                    Ok(r) => {
                        if errors == 0 {
                            eprintln!(
                                "first error: {} {}",
                                r.status(),
                                r.text().await.unwrap_or_default()
                            );
                        }
                        errors += 1;
                    }
                    Err(e) => {
                        if errors == 0 {
                            eprintln!("first error: {e}");
                        }
                        errors += 1;
                    }
                }
            }
            (hist, max_us, errors)
        }));
    }
    let progress_runtime = Arc::clone(&runtime);
    let progress_task = progress.then(|| {
        tokio::spawn(async move {
            let mut last = 0u64;
            loop {
                tokio::time::sleep(Duration::from_secs(10)).await;
                let g = progress_runtime.ledger().gauges.snapshot();
                let rss = std::fs::read_to_string("/proc/self/status")
                    .ok()
                    .and_then(|s| {
                        s.lines()
                            .find(|l| l.starts_with("VmRSS:"))
                            .map(|l| l.trim_start_matches("VmRSS:").trim().to_owned())
                    })
                    .unwrap_or_else(|| "n/a".into());
                eprintln!(
                    "[{:>5}s] worlds {} (+{}) live {} ops {} rss {}",
                    started.elapsed().as_secs(),
                    g.worlds_created,
                    g.worlds_created - last,
                    g.live_worlds,
                    g.live_ops,
                    rss
                );
                last = g.worlds_created;
            }
        })
    });
    let mut hist = vec![0u64; BUCKETS + 1];
    let mut max_us = 0u64;
    let mut errors = 0;
    for w in workers {
        let (h, m, e) = w.await?;
        for (a, b) in hist.iter_mut().zip(h) {
            *a += b;
        }
        max_us = max_us.max(m);
        errors += e;
    }
    if let Some(t) = progress_task {
        t.abort();
    }
    let elapsed = started.elapsed();
    let total: u64 = hist.iter().sum();
    let pct = |p: f64| -> f64 {
        if total == 0 {
            return 0.0;
        }
        if p >= 100.0 {
            return max_us as f64 / 1000.0;
        }
        let rank = ((p / 100.0) * total as f64).ceil().max(1.0) as u64;
        let mut seen = 0u64;
        for (i, n) in hist.iter().enumerate() {
            seen += n;
            if seen >= rank {
                return ((i as u64 + 1) * BUCKET_US) as f64 / 1000.0;
            }
        }
        max_us as f64 / 1000.0
    };
    shutdown.cancel();
    runtime.shutdown().await;
    let _ = tokio::time::timeout(Duration::from_secs(10), server).await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    let g = runtime.ledger().gauges.snapshot();
    let rss = std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("VmHWM:"))
                .map(|l| l.trim_start_matches("VmHWM:").trim().to_owned())
        })
        .unwrap_or_else(|| "n/a".into());
    println!("usai bench (engineering measurement, not canonical evidence)\n");
    println!(
        "target        {path}\nconcurrency   {concurrency}\nduration      {:.1}s",
        elapsed.as_secs_f64()
    );
    println!(
        "requests      {} ok, {} errors ({:.0} req/s)",
        total,
        errors,
        total as f64 / elapsed.as_secs_f64()
    );
    println!(
        "latency ms    p50 {:.2}  p90 {:.2}  p99 {:.2}  max {:.2}",
        pct(50.0),
        pct(90.0),
        pct(99.0),
        pct(100.0)
    );
    println!(
        "worlds        {} created, {} live after drain, {} live ops",
        g.worlds_created, g.live_worlds, g.live_ops
    );
    println!("rss high-water {rss}");
    if g.live_worlds != 0 || g.live_ops != 0 {
        anyhow::bail!("ownership did not return to baseline after the run");
    }
    Ok(())
}

/// `usai test`: builds the project once (so the first `testApp` is fast),
/// then runs `node --test` with USAI_BIN pointing at this binary.
pub async fn test(root: &Path, args: Vec<String>) -> Result<()> {
    let engine = engine();
    let config = load_config(engine.as_ref(), root).await?;
    usai_runtime::build::build(engine.as_ref(), &BuildOptions::from_config(&config)).await?;
    let me = std::env::current_exe().context("cannot locate the usai binary")?;
    let mut command = tokio::process::Command::new("node");
    // In this repository's workspace the SDK is a symlink to its sources and
    // the `usai` export condition lets Node run them without a `dist/`. From
    // a published package (`node_modules/@sakaladev/usai` is a real
    // directory) Node refuses to strip types, so the condition must stay
    // off and `dist/` is used.
    let sdk = config.root.join("node_modules/@sakaladev/usai");
    let workspace_sdk = std::fs::symlink_metadata(&sdk)
        .map(|m| m.file_type().is_symlink())
        .unwrap_or(false)
        && sdk.join("src/index.ts").exists()
        && !std::fs::canonicalize(&sdk)
            .map(|real| real.components().any(|c| c.as_os_str() == "node_modules"))
            .unwrap_or(true);
    if workspace_sdk {
        command.arg("--conditions=usai");
    }
    command.arg("--test");
    if args.is_empty() {
        for pattern in [
            "src/**/*.test.ts",
            "test/**/*.test.ts",
            "tests/**/*.test.ts",
        ] {
            command.arg(pattern);
        }
    } else {
        command.args(&args);
    }
    let status = command
        .current_dir(&config.root)
        .env("USAI_BIN", &me)
        .status()
        .await
        .context("node is required to run tests")?;
    if !status.success() {
        anyhow::bail!("tests failed");
    }
    Ok(())
}
