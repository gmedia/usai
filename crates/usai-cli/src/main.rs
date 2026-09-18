//! `usai` — the command-line tool. Every command reads the same
//! ApplicationDefinition the runtime executes (contract C8).

mod commands;
mod display;

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
        /// Serve /_usai/status and /_usai/metrics
        #[arg(long)]
        status: bool,
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
    /// Database: migrations and seeders
    Db {
        #[command(subcommand)]
        action: DbAction,
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

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    // Servers narrate (revisions, images, listeners); one-shot commands print
    // their result and stay quiet unless asked.
    let serves = matches!(
        cli.command,
        Command::Run { .. } | Command::Dev { .. } | Command::Bench { .. }
    );
    let default_filter = if serves || cli.verbose {
        "usai=info,usai_runtime=info"
    } else {
        "usai=warn,usai_runtime=warn"
    };
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| default_filter.into()),
        )
        .with_target(false)
        .compact()
        .init();
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
            control,
            announce,
            require_signature,
        } => {
            commands::run(
                &root,
                &host,
                port,
                artifact,
                status,
                control,
                announce,
                require_signature,
            )
            .await
        }
        Command::Dev { host, port } => commands::dev(&root, &host, port).await,
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
            action: GenerateAction::Openapi { out },
        } => commands::generate_openapi(&root, out).await,
    };
    if let Err(error) = result {
        eprintln!("error: {error:#}");
        std::process::exit(1);
    }
}
