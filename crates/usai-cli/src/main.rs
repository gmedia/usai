//! `usai` — the command-line tool. Every command reads the same
//! ApplicationDefinition the runtime executes (contract C8).

mod commands;
mod display;
mod logfmt;

use std::path::PathBuf;
use std::time::Duration;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "usai",
    version,
    about = "Usai — a lifecycle-native application runtime"
)]
struct Cli {
    /// Project root (defaults to the current directory)
    #[arg(long, global = true)]
    root: Option<PathBuf>,
    /// Log format: text (default) or json
    #[arg(long, global = true, default_value = "text")]
    log_format: String,
    /// Execution substrate: wasm (default) or quickjs; $USAI_ENGINE also works
    #[arg(long, global = true)]
    engine: Option<String>,
    /// Show the runtime's INFO logs for one-shot commands too (RUST_LOG overrides)
    #[arg(long, short = 'v', global = true)]
    verbose: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Build the application artifact (manifest + bundled code)
    Build {
        /// Skip the TypeScript check (`tsc --noEmit`) that runs when the
        /// project has a tsconfig.json and typescript installed
        #[arg(long)]
        no_typecheck: bool,
        /// Sign the artifact with this Ed25519 key file (`usai keygen`);
        /// USAI_SIGNING_KEY is the equivalent environment variable
        #[arg(long, env = "USAI_SIGNING_KEY")]
        sign: Option<PathBuf>,
    },
    /// Generate an Ed25519 signing key for `build --sign`; prints the public key
    Keygen {
        /// Where to write the private key (hex seed); default usai-signing.key
        #[arg(long, default_value = "usai-signing.key")]
        out: PathBuf,
    },
    /// Serve a built artifact (builds first when none exists)
    Run {
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
        #[arg(long, short, default_value_t = 3000)]
        port: u16,
        /// Artifact directory (defaults to the configured outDir)
        #[arg(long)]
        artifact: Option<PathBuf>,
        /// Serve /_usai/status, /_usai/metrics, /_usai/live, /_usai/ready and the API
        /// docs on the application listener (development and trusted networks)
        #[arg(long)]
        status: bool,
        /// Serve those surfaces on their own listener instead (a private
        /// interface: 127.0.0.1:9090, a pod-local address); USAI_STATUS_ADDR is the environment form
        #[arg(long, env = "USAI_STATUS_ADDR")]
        status_addr: Option<String>,
        /// Require `Authorization: Bearer <token>` on /_usai/status, /_usai/metrics
        /// and /_usai/openapi.json (both listeners; /_usai/live and /_usai/ready stay
        /// open for probes, the /_usai/docs shell asks for the token itself).
        /// USAI_STATUS_TOKEN is the environment form — prefer it: a flag shows in `ps`
        #[arg(long, env = "USAI_STATUS_TOKEN", hide_env_values = true)]
        status_token: Option<String>,
        /// Switch operator surfaces off by name, comma-separated: status, metrics,
        /// docs (the reference and /_usai/openapi.json), live, ready — on both
        /// listeners; a switched-off surface is a 404. USAI_SURFACES_OFF is the
        /// environment form (e.g. `docs,metrics` when neither is consumed)
        #[arg(long, env = "USAI_SURFACES_OFF", value_delimiter = ',')]
        surfaces_off: Vec<String>,
        /// Bind the local control surface (install/activate/drain/stop), e.g. 127.0.0.1:3900.
        /// Token from USAI_CONTROL_TOKEN (required off loopback).
        #[arg(long)]
        control: Option<String>,
        /// Print one JSON line with the bound addresses on stdout (for harnesses)
        #[arg(long)]
        announce: bool,
        /// Refuse artifacts not signed by one of these public keys (hex, or a
        /// file holding one per line); USAI_REQUIRE_SIGNATURE is the environment form
        #[arg(long, env = "USAI_REQUIRE_SIGNATURE", value_delimiter = ',')]
        require_signature: Vec<String>,
        /// Execution worlds this instance holds at once (the admission bound;
        /// each reserves memory up front). USAI_MAX_WORLDS is the environment form
        #[arg(long, env = "USAI_MAX_WORLDS", default_value_t = 256)]
        max_worlds: u32,
        /// Do not run the cron scheduler on this instance. Every instance
        /// that runs it ticks every schedule; with several replicas, keep it
        /// on exactly one (USAI_NO_CRON=1 on the others)
        #[arg(long, env = "USAI_NO_CRON", value_parser = clap::builder::FalseyValueParser::new())]
        no_cron: bool,
        /// Do not run queue consumers on this instance (USAI_NO_QUEUE=1);
        /// consumers on several replicas share the work safely, so this is
        /// for dedicating replicas, not for correctness
        #[arg(long, env = "USAI_NO_QUEUE", value_parser = clap::builder::FalseyValueParser::new())]
        no_queue: bool,
        /// Do not run the application's services on this instance
        /// (USAI_NO_SERVICES=1). Every instance that runs them has its own
        /// copy of each service; with several replicas, keep them where
        /// you mean them. One-shot commands never run services
        #[arg(long, env = "USAI_NO_SERVICES", value_parser = clap::builder::FalseyValueParser::new())]
        no_services: bool,
        /// How long a shutdown or a revision replacement waits for in-flight
        /// work before cancelling it, in seconds (USAI_DRAIN_TIMEOUT). Set the
        /// orchestrator's grace period above this
        #[arg(long, env = "USAI_DRAIN_TIMEOUT", default_value_t = 30)]
        drain_timeout: u64,
        /// On SIGTERM, keep serving for this many seconds while /_usai/ready
        /// answers 503 and responses carry `Connection: close`, so a load
        /// balancer stops routing here before the listener closes; then drain
        /// (USAI_DRAIN_GRACE). Set it at or above the balancer's health-check
        /// interval; 0 closes the listener at once
        #[arg(long, env = "USAI_DRAIN_GRACE", default_value_t = 2)]
        drain_grace: u64,
        /// Expose diagnostics to clients as `usai dev` does: error details in
        /// 500 bodies and the x-usai-lifecycle / x-usai-server-ms headers.
        /// For tests and trusted networks only (USAI_DIAGNOSTICS=1)
        #[arg(long, env = "USAI_DIAGNOSTICS", value_parser = clap::builder::FalseyValueParser::new())]
        diagnostics: bool,
    },
    /// Build, serve, and rebuild on change as a new revision
    Dev {
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
        #[arg(long, short, default_value_t = 3000)]
        port: u16,
    },
    /// Show the application Usai understood
    Inspect {
        #[arg(long)]
        json: bool,
    },
    /// Show the workload → resource / dispatch graph
    Graph,
    /// Show the effective project configuration and where each value came from
    Config {
        #[arg(long)]
        json: bool,
    },
    /// Run a user-defined command in a fresh finite world
    App {
        /// Command name as declared with `command("<name>", …)`
        name: String,
        /// Arguments passed to the command
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Cron utilities
    Cron {
        #[command(subcommand)]
        action: CronAction,
    },
    /// Task utilities
    Task {
        #[command(subcommand)]
        action: TaskAction,
    },
    /// Queue utilities
    Queue {
        #[command(subcommand)]
        action: QueueAction,
    },
    /// Database: migrations and seeders
    Db {
        #[command(subcommand)]
        action: DbAction,
    },
    /// Probe a running instance's liveness or readiness and exit 0 / 1: a
    /// container healthcheck without curl in the image
    Probe {
        /// `live` or `ready`
        which: String,
        /// The listener that serves /_usai/* (`--status-addr`, or the app
        /// listener with `--status`); USAI_STATUS_ADDR is the environment form
        #[arg(long, env = "USAI_STATUS_ADDR", default_value = "127.0.0.1:9090")]
        addr: String,
        /// Give up after this many seconds (default 3). An exec probe's own
        /// timeout must be above it, or the supervisor kills the check
        /// rather than reading its answer.
        #[arg(long, default_value_t = 3)]
        timeout: u64,
    },
    /// Run the project's tests with `usai/test` pointed at this binary
    Test {
        /// Arguments passed to `node --test` (default: the project's test files)
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Engineering load test against an in-process server (not evidence)
    Bench {
        #[arg(long, default_value = "/")]
        path: String,
        #[arg(long, short, default_value_t = 16)]
        concurrency: usize,
        /// Seconds
        #[arg(long, short, default_value_t = 10)]
        duration: u64,
    },
    /// Generate artifacts from the application definition
    Generate {
        #[command(subcommand)]
        action: GenerateAction,
    },
}

#[derive(Subcommand)]
enum GenerateAction {
    /// OpenAPI 3.1 document
    Openapi {
        /// Write to a file instead of stdout
        #[arg(long)]
        out: Option<PathBuf>,
        /// The consumer contract only: no x-usai-* extensions, no workload,
        /// resource or environment inventory (what ships to API consumers)
        #[arg(long)]
        public: bool,
    },
}

#[derive(Subcommand)]
enum DbAction {
    /// Apply pending migrations in order
    Migrate {
        /// Postgres resource to migrate (default: the first declared)
        #[arg(long)]
        resource: Option<String>,
        /// Use a built artifact's migrations (production image, no source)
        #[arg(long)]
        artifact: Option<PathBuf>,
    },
    /// Show applied and pending migrations
    Status {
        #[arg(long)]
        resource: Option<String>,
        #[arg(long)]
        json: bool,
        /// Use a built artifact's migrations (production image, no source)
        #[arg(long)]
        artifact: Option<PathBuf>,
    },
    /// Run seeders (all, or one by name)
    Seed { name: Option<String> },
}

#[derive(Subcommand)]
enum CronAction {
    /// Run one invocation now, without waiting for the schedule
    Run { name: String },
}

#[derive(Subcommand)]
enum TaskAction {
    /// Run a task once with a JSON input
    Run {
        name: String,
        #[arg(long, default_value = "null")]
        input: String,
    },
}

#[derive(Subcommand)]
enum QueueAction {
    /// Deliver one JSON message to a topic's consumer directly (a fresh
    /// world, attempt 1, nothing written to the queue table, no retry)
    Run {
        topic: String,
        #[arg(long, default_value = "null")]
        message: String,
    },
    /// What is in `usai_queue`: rows per topic and state, the oldest message
    /// still waiting and the oldest still claimed
    Status {
        /// Only this topic
        #[arg(long)]
        topic: Option<String>,
        /// The postgres resource to use (default: the application's first)
        #[arg(long)]
        resource: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Delete finished rows. The runtime never prunes by itself: a `dead`
    /// row is a message your application failed, and only you know whether
    /// it is still evidence
    Prune {
        /// `done`, `dead` or `all`
        #[arg(long, default_value = "done")]
        state: String,
        /// Only rows created longer ago than this (`7d`, `48h`, `30m`)
        #[arg(long, default_value = "7d")]
        older_than: String,
        /// Only this topic
        #[arg(long)]
        topic: Option<String>,
        #[arg(long)]
        resource: Option<String>,
        /// Count what would go, delete nothing
        #[arg(long)]
        dry_run: bool,
    },
    /// Create the queue's indexes with CREATE INDEX CONCURRENTLY, before an
    /// upgrade builds them under a write-blocking lock. Safe to run while
    /// the application serves, and safe to repeat
    Prepare {
        #[arg(long)]
        resource: Option<String>,
    },
}

/// `KEY=VALUE` lines (optional `export`, optional single/double quotes,
/// `#` comments), for the variables the shell did not already set.
pub fn parse_dotenv(path: &std::path::Path) -> Vec<(String, String)> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").unwrap_or(line);
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        if key.is_empty() || std::env::var_os(key).is_some() {
            continue;
        }
        let value = value.trim();
        let value = value
            .strip_prefix('"')
            .and_then(|v| v.strip_suffix('"'))
            .or_else(|| value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')))
            .unwrap_or(value);
        out.push((key.to_owned(), value.to_owned()));
    }
    out
}

/// Sets the `.env` variables into the process environment; returns how many.
fn load_dotenv(path: &std::path::Path) -> usize {
    let vars = parse_dotenv(path);
    for (key, value) in &vars {
        // SAFETY: called at startup before the command runs; nothing else
        // reads the environment concurrently yet.
        unsafe { std::env::set_var(key, value) };
    }
    vars.len()
}

/// glibc grows one malloc arena per thread that allocates concurrently; a
/// 10 MB module deserialized on worker thread N leaves a 10 MB free chunk
/// in arena N that arena M cannot reuse. With 16 workers and revision churn
/// that is 16 × (image + bookkeeping) of retained-but-free memory — the P6
/// campaign hit a 512 MB limit after ~270 replacements. Two arenas bound it.
#[cfg(all(target_os = "linux", target_env = "gnu"))]
fn bound_malloc_arenas() {
    // SAFETY: called before any thread is spawned; plain libc setting.
    unsafe {
        libc::mallopt(libc::M_ARENA_MAX, 2);
    }
}

#[cfg(not(all(target_os = "linux", target_env = "gnu")))]
fn bound_malloc_arenas() {}

fn main() {
    bound_malloc_arenas();
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime")
        .block_on(async_main());
}

async fn async_main() {
    let cli = Cli::parse();
    // Servers narrate (revisions, images, listeners); one-shot commands print
    // their result and stay quiet unless asked.
    let serves = matches!(
        cli.command,
        Command::Run { .. } | Command::Dev { .. } | Command::Bench { .. }
    );
    // `app` is the application's console/ctx.log; the compiler's internals
    // (cranelift, wasmtime) stay off unless RUST_LOG names them — at debug
    // they emit ~100 000 lines per image.
    let default_filter = if serves || cli.verbose {
        "usai=info,usai_runtime=info,app=info"
    } else {
        "usai=warn,usai_runtime=warn,app=info"
    };
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| default_filter.into())
        .add_directive("cranelift_codegen=warn".parse().expect("directive"))
        .add_directive("cranelift_frontend=warn".parse().expect("directive"))
        .add_directive("wasmtime_cranelift=warn".parse().expect("directive"))
        .add_directive("wasmtime=warn".parse().expect("directive"));
    // Logs go to stderr, results (inspect, app, generate) to stdout, so a
    // command's output can be piped while the runtime narrates.
    match cli.log_format.as_str() {
        // JSON lines keep the target so `target == "app"` selects the
        // application's own lines (`ctx.log`) from the runtime's.
        "json" => tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_writer(std::io::stderr)
            .event_format(logfmt::JsonLine)
            .init(),
        "text" => tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_writer(std::io::stderr)
            .with_target(false)
            // Colour only when a person is looking (stderr is a terminal).
            .with_ansi(std::io::IsTerminal::is_terminal(&std::io::stderr()))
            .compact()
            .init(),
        other => {
            eprintln!("error: --log-format {other}: expected text or json");
            std::process::exit(2);
        }
    }
    if cli.log_format == "json" {
        // A JSON log pipeline reads both of the process's streams under an
        // orchestrator: the human banner on stdout would be a parse error per
        // start. `serve` emits one structured line instead.
        commands::suppress_human_output();
    }
    if let Some(engine) = &cli.engine {
        // SAFETY: no other thread exists yet; the runtime reads it later.
        unsafe { std::env::set_var("USAI_ENGINE", engine) };
    }
    let root = cli
        .root
        .map(|r| std::path::absolute(&r).unwrap_or(r))
        .unwrap_or_else(|| std::env::current_dir().expect("cwd"));
    // Local development reads `<root>/.env` (never overriding what the shell
    // already set); `usai run` does not — production configuration comes
    // from the deployment environment, on purpose. `usai dev` reads it on
    // every rebuild instead (see `commands::dev`), so it is not set here.
    if !matches!(cli.command, Command::Run { .. } | Command::Dev { .. }) {
        load_dotenv(&root.join(".env"));
    }
    let result = match cli.command {
        Command::Build { no_typecheck, sign } => commands::build(&root, !no_typecheck, sign).await,
        Command::Keygen { out } => commands::keygen(&out),
        Command::Run {
            host,
            port,
            artifact,
            status,
            status_addr,
            status_token,
            surfaces_off,
            control,
            announce,
            require_signature,
            max_worlds,
            no_cron,
            no_queue,
            no_services,
            drain_timeout,
            drain_grace,
            diagnostics,
        } => {
            // The flag and the environment variable are the same setting;
            // `run` reads it from the environment where the listener is built.
            if let Some(token) = status_token {
                // SAFETY: single-threaded at this point (before the runtime starts).
                unsafe { std::env::set_var("USAI_STATUS_TOKEN", token) };
            }
            if !surfaces_off.is_empty() {
                for name in &surfaces_off {
                    if !["status", "metrics", "docs", "live", "ready"].contains(&name.as_str()) {
                        eprintln!(
                            "error: --surfaces-off {name}: expected status, metrics, docs, live or ready"
                        );
                        std::process::exit(2);
                    }
                }
                // SAFETY: as above.
                unsafe { std::env::set_var("USAI_SURFACES_OFF", surfaces_off.join(",")) };
            }
            commands::run(
                &root,
                &host,
                port,
                artifact,
                status,
                status_addr,
                control,
                announce,
                require_signature,
                max_worlds,
                no_cron,
                no_queue,
                no_services,
                drain_timeout,
                drain_grace,
                diagnostics,
            )
            .await
        }
        Command::Dev { host, port } => commands::dev(&root, &host, port).await,
        Command::Probe {
            which,
            addr,
            timeout,
        } => commands::probe(&which, &addr, timeout).await,
        Command::Inspect { json } => commands::inspect(&root, json).await,
        Command::Graph => commands::graph(&root).await,
        Command::Config { json } => commands::config(&root, json).await,
        Command::App { name, args } => commands::app(&root, &name, args).await,
        Command::Cron {
            action: CronAction::Run { name },
        } => commands::cron_run(&root, &name).await,
        Command::Task {
            action: TaskAction::Run { name, input },
        } => commands::task_run(&root, &name, &input).await,
        Command::Queue {
            action: QueueAction::Run { topic, message },
        } => commands::queue_run(&root, &topic, &message).await,
        Command::Queue {
            action:
                QueueAction::Status {
                    topic,
                    resource,
                    json,
                },
        } => commands::queue_status(&root, topic.as_deref(), resource.as_deref(), json).await,
        Command::Queue {
            action:
                QueueAction::Prune {
                    state,
                    older_than,
                    topic,
                    resource,
                    dry_run,
                },
        } => {
            commands::queue_prune(
                &root,
                &state,
                &older_than,
                topic.as_deref(),
                resource.as_deref(),
                dry_run,
            )
            .await
        }
        Command::Queue {
            action: QueueAction::Prepare { resource },
        } => commands::queue_prepare(&root, resource.as_deref()).await,
        Command::Db {
            action: DbAction::Migrate { resource, artifact },
        } => commands::db_migrate(&root, resource.as_deref(), artifact).await,
        Command::Db {
            action:
                DbAction::Status {
                    resource,
                    json,
                    artifact,
                },
        } => commands::db_status(&root, resource.as_deref(), json, artifact).await,
        Command::Db {
            action: DbAction::Seed { name },
        } => commands::db_seed(&root, name.as_deref()).await,
        Command::Test { args } => commands::test(&root, args).await,
        Command::Bench {
            path,
            concurrency,
            duration,
        } => commands::bench(&root, &path, concurrency, Duration::from_secs(duration)).await,
        Command::Generate {
            action: GenerateAction::Openapi { out, public },
        } => commands::generate_openapi(&root, out, public).await,
    };
    if let Err(error) = result {
        eprintln!("error: {error:#}");
        std::process::exit(1);
    }
}
