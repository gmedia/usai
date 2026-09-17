//! `usai` — the command-line tool. Every command reads the same
//! ApplicationDefinition the runtime executes (contract C8).

mod commands;
mod display;

use std::path::PathBuf;

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
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Build the application artifact (manifest + bundled code)
    Build,
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
    },
    /// Show applied and pending migrations
    Status {
        #[arg(long)]
        resource: Option<String>,
        #[arg(long)]
        json: bool,
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

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "usai=info,usai_runtime=info".into()),
        )
        .with_target(false)
        .compact()
        .init();
    let cli = Cli::parse();
    let root = cli
        .root
        .map(|r| std::path::absolute(&r).unwrap_or(r))
        .unwrap_or_else(|| std::env::current_dir().expect("cwd"));
    let result = match cli.command {
        Command::Build => commands::build(&root).await,
        Command::Run {
            host,
            port,
            artifact,
            status,
        } => commands::run(&root, &host, port, artifact, status).await,
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
            action: DbAction::Migrate { resource },
        } => commands::db_migrate(&root, resource.as_deref()).await,
        Command::Db {
            action: DbAction::Status { resource, json },
        } => commands::db_status(&root, resource.as_deref(), json).await,
        Command::Db {
            action: DbAction::Seed { name },
        } => commands::db_seed(&root, name.as_deref()).await,
        Command::Generate {
            action: GenerateAction::Openapi { out },
        } => commands::generate_openapi(&root, out).await,
    };
    if let Err(error) = result {
        eprintln!("error: {error:#}");
        std::process::exit(1);
    }
}
