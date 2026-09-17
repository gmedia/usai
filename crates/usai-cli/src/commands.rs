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

fn engine() -> Arc<QuickJsEngine> {
    QuickJsEngine::new(QuickJsConfig::default())
}

pub async fn build(root: &Path) -> Result<()> {
    let engine = engine();
    let config = load_config(engine.as_ref(), root).await?;
    let started = std::time::Instant::now();
    let out =
        usai_runtime::build::build(engine.as_ref(), &BuildOptions::from_config(&config)).await?;
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
    Ok(())
}

async fn definition_for(
    root: &Path,
    artifact: Option<PathBuf>,
) -> Result<(Arc<ApplicationDefinition>, Arc<QuickJsEngine>)> {
    let engine = engine();
    let config = load_config(engine.as_ref(), root).await?;
    let dir = artifact.unwrap_or_else(|| config.out_dir.value.clone());
    let definition = if dir.join("manifest.json").exists() {
        load_artifact(&dir).await?
    } else {
        usai_runtime::build::build(engine.as_ref(), &BuildOptions::from_config(&config))
            .await?
            .definition
    };
    Ok((definition, engine))
}

pub async fn run(
    root: &Path,
    host: &str,
    port: u16,
    artifact: Option<PathBuf>,
    status: bool,
) -> Result<()> {
    let (definition, engine) = definition_for(root, artifact).await?;
    let runtime = Runtime::new(engine, RuntimeConfig::default());
    let revision = runtime.install(definition).await?;
    runtime.activate(revision.id).await?;
    serve_until_signal(runtime, host, port, false, status, None).await
}

pub async fn graph(root: &Path) -> Result<()> {
    let (definition, _) = definition_for(root, None).await?;
    print!("{}", usai_runtime::observability::render_graph(&definition));
    Ok(())
}

async fn serve_until_signal(
    runtime: Arc<Runtime>,
    host: &str,
    port: u16,
    expose_diagnostics: bool,
    serve_status: bool,
    on_ready: Option<Box<dyn FnOnce(String) + Send>>,
) -> Result<()> {
    let addr: std::net::SocketAddr = format!("{host}:{port}")
        .parse()
        .context("invalid listen address")?;
    let http = HttpHost::new(
        Arc::clone(&runtime),
        HttpConfig {
            addr,
            expose_diagnostics,
            serve_docs: expose_diagnostics,
            serve_status,
            ..HttpConfig::default()
        },
    );
    let shutdown = CancellationToken::new();
    let server = {
        let shutdown = shutdown.clone();
        let (tx, rx) = tokio::sync::oneshot::channel();
        let handle = tokio::spawn(async move {
            serve(http, shutdown, |addr| {
                let _ = tx.send(addr);
            })
            .await
        });
        let bound = rx.await.context("server did not bind")?;
        let url = format!("http://{bound}");
        match on_ready {
            Some(f) => f(url),
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
                    )
                );
            }
        }
        handle
    };
    tokio::signal::ctrl_c().await.ok();
    eprintln!("\nshutting down: draining in-flight work (ctrl-c again to force)");
    shutdown.cancel();
    let drain = async {
        runtime.shutdown().await;
        let _ = tokio::time::timeout(Duration::from_secs(35), server).await;
    };
    tokio::select! {
        _ = drain => eprintln!("drained; ownership returned to baseline"),
        _ = tokio::signal::ctrl_c() => {
            let g = runtime.ledger().gauges.snapshot();
            eprintln!("forced shutdown with {} live worlds and {} live operations", g.live_worlds, g.live_ops);
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
        print!("{}", display::inspect(&definition));
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
    let engine = engine();
    let config = load_config(engine.as_ref(), root).await?;
    let options = BuildOptions::from_config(&config);
    let first = usai_runtime::build::build(engine.as_ref(), &options).await?;
    let runtime = Runtime::new(
        Arc::clone(&engine) as Arc<dyn usai_runtime::engine::Engine>,
        RuntimeConfig::default(),
    );
    let revision = runtime.install(first.definition).await?;
    runtime.activate(revision.id).await?;

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
            let relevant = {
                let paths = watcher_paths.lock().expect("watched poisoned");
                event.paths.iter().any(|p| {
                    paths.iter().any(|w| w == p)
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
    if let Some(config_file) = &config.config_file {
        watcher.watch(config_file, notify::RecursiveMode::NonRecursive)?;
    }

    let rebuild_runtime = Arc::clone(&runtime);
    let rebuild_engine = Arc::clone(&engine);
    let rebuild_root = root.to_path_buf();
    tokio::spawn(async move {
        while rx.recv().await.is_some() {
            // Coalesce bursts of filesystem events.
            tokio::time::sleep(Duration::from_millis(120)).await;
            while rx.try_recv().is_ok() {}
            let started = std::time::Instant::now();
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
                    let previous = rebuild_runtime.active().ok();
                    match rebuild_runtime.install(out.definition).await {
                        Ok(rev) => match rebuild_runtime.activate(rev.id).await {
                            Ok(rev) => {
                                eprintln!(
                                    "\nrevision {} active ({}) in {:?}",
                                    rev.id,
                                    rev.definition.identity(),
                                    started.elapsed()
                                );
                                if let Some(prev) = previous {
                                    let rt = Arc::clone(&rebuild_runtime);
                                    tokio::spawn(async move {
                                        if let Err(e) = rt.drain(prev.id).await {
                                            eprintln!("drain of {} failed: {e}", prev.id);
                                        }
                                    });
                                }
                            }
                            Err(e) => eprintln!(
                                "\nactivation failed: {e:#}\n(previous revision keeps serving)"
                            ),
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
        host,
        port,
        true,
        true,
        Some(Box::new(move |url| {
            let revision = banner_runtime.active().expect("active");
            print!(
                "{}",
                display::banner(
                    &revision.definition,
                    &format!("{} ({})", revision.id, revision.definition.identity()),
                    Some(&url),
                    Some(&banner_runtime.status()),
                    true,
                )
            );
            println!("\nwatching for changes (ctrl-c to stop)");
        })),
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
            ..RuntimeConfig::default()
        },
    );
    let revision = runtime.install(definition).await?;
    runtime.activate(revision.id).await?;
    let result = run(&runtime).await;
    runtime.shutdown().await;
    let result = result?;
    for line in &result.logs {
        eprintln!("[{}] {}", line.level, line.message);
    }
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
            ..RuntimeConfig::default()
        },
    );
    let revision = runtime.install(out.definition).await?;
    runtime.activate(revision.id).await?;
    let result = f(&runtime, &config).await;
    runtime.shutdown().await;
    result
}

pub async fn db_migrate(root: &Path, resource: Option<&str>) -> Result<()> {
    with_active_runtime(root, async |runtime, config| {
        let revision = runtime.active()?;
        let globs = db::migration_globs(&revision.definition, &config.migrations.value);
        let files = db::discover_migrations(&config.root, &globs)?;
        let manager = db::database(&revision, resource)?;
        let applied = db::migrate(manager.as_ref(), &files, CancellationToken::new()).await?;
        if applied.is_empty() {
            println!(
                "nothing to apply ({} migrations already applied)",
                files.len()
            );
        } else {
            for name in &applied {
                println!("applied {name}");
            }
        }
        Ok(())
    })
    .await
}

pub async fn db_status(root: &Path, resource: Option<&str>, json: bool) -> Result<()> {
    with_active_runtime(root, async |runtime, config| {
        let revision = runtime.active()?;
        let globs = db::migration_globs(&revision.definition, &config.migrations.value);
        let files = db::discover_migrations(&config.root, &globs)?;
        let manager = db::database(&revision, resource)?;
        let status = db::status(manager.as_ref(), &files).await?;
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
            Arc::clone(&engine) as Arc<dyn usai_runtime::engine::Engine>,
            RuntimeConfig {
                cron_scheduler: false,
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
        for line in &result.logs {
            eprintln!("[{}] {}", line.level, line.message);
        }
        match (&result.termination, &result.outcome) {
            (Termination::Completed, Some(Ok(_))) => println!(
                "seeded {} ({})",
                seeder.name,
                seeder
                    .path
                    .strip_prefix(&config.root)
                    .unwrap_or(&seeder.path)
                    .display()
            ),
            (Termination::Completed, Some(Err(e))) => {
                anyhow::bail!("seeder {} failed: {}: {}", seeder.name, e.name, e.message)
            }
            (t, _) => anyhow::bail!("seeder {} ended without a result: {t:?}", seeder.name),
        }
    }
    Ok(())
}

pub async fn generate_openapi(root: &Path, out: Option<PathBuf>) -> Result<()> {
    let (definition, _) = definition_for(root, None).await?;
    let document = usai_runtime::openapi::generate(&definition);
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
    let (definition, engine) = definition_for(root, None).await?;
    let runtime = Runtime::new(
        engine,
        RuntimeConfig {
            cron_scheduler: false,
            queue_consumers: false,
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
    for _ in 0..concurrency {
        let client = client.clone();
        let url = url.clone();
        workers.push(tokio::spawn(async move {
            let mut latencies = Vec::new();
            let mut errors = 0u64;
            while std::time::Instant::now() < deadline {
                let t = std::time::Instant::now();
                match client.get(&url).send().await {
                    Ok(r) if r.status().is_success() => {
                        let _ = r.bytes().await;
                        latencies.push(t.elapsed().as_micros() as u64);
                    }
                    _ => errors += 1,
                }
            }
            (latencies, errors)
        }));
    }
    let mut all = Vec::new();
    let mut errors = 0;
    for w in workers {
        let (l, e) = w.await?;
        all.extend(l);
        errors += e;
    }
    let elapsed = started.elapsed();
    all.sort_unstable();
    let pct = |p: f64| -> f64 {
        if all.is_empty() {
            return 0.0;
        }
        let rank = ((p / 100.0) * all.len() as f64).ceil().max(1.0) as usize;
        all[rank.min(all.len()) - 1] as f64 / 1000.0
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
        all.len(),
        errors,
        all.len() as f64 / elapsed.as_secs_f64()
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
