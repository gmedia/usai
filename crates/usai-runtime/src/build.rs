//! The build phase (ADR-0005, ADR-0009).
//!
//! ```text
//! entry.ts ──esbuild──▶ app.js ──engine.compile──▶ bytecode
//!                         │                          │
//!                         └──── describe() in a capability-less world ──▶ manifest.json
//! ```
//!
//! The bundler is the `usai` npm package's `build/bundle.mjs`, resolved from
//! the project, so the SDK and its bundler always agree. The manifest comes
//! from evaluating declarations only; no host operation is available.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Deserialize;

use crate::definition::{ApplicationDefinition, Code, DefinitionError, Manifest};
use crate::engine::{Engine, EngineError};

#[derive(Clone, Debug)]
pub struct BuildOptions {
    /// Project root (where `node_modules/usai` resolves from).
    pub root: PathBuf,
    /// Application entry, relative to `root` or absolute.
    pub entry: PathBuf,
    /// Output directory for `app.js` and `manifest.json`.
    pub out_dir: PathBuf,
}

impl BuildOptions {
    pub fn for_project(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        Self {
            entry: root.join("src/app.ts"),
            out_dir: root.join(".usai/build"),
            root,
        }
    }
}

pub struct BuildOutput {
    pub definition: Arc<ApplicationDefinition>,
    pub manifest_path: PathBuf,
    pub code_path: PathBuf,
    /// Source files the bundle depends on, for the dev watcher.
    pub inputs: Vec<PathBuf>,
}

#[derive(Debug, thiserror::Error)]
pub enum BuildError {
    #[error("node is required to build a Usai application but was not found on PATH")]
    NodeMissing,
    #[error(
        "could not resolve the `usai` package from {root}: {detail}\n  hint: run `pnpm add usai` (or npm install) in the project"
    )]
    SdkMissing { root: PathBuf, detail: String },
    #[error("bundling failed:\n{0}")]
    Bundle(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Engine(#[from] EngineError),
    #[error("manifest produced by the SDK is invalid: {0}")]
    Manifest(#[from] serde_json::Error),
    #[error(transparent)]
    Definition(#[from] DefinitionError),
}

#[derive(Deserialize)]
struct BundleReport {
    ok: bool,
    #[serde(default)]
    inputs: Vec<String>,
    #[serde(default)]
    errors: Vec<BundleMessage>,
}

#[derive(Deserialize)]
struct BundleMessage {
    text: String,
    #[serde(default)]
    location: Option<BundleLocation>,
}

#[derive(Deserialize)]
struct BundleLocation {
    file: String,
    line: u32,
    column: u32,
}

async fn node_available() -> bool {
    tokio::process::Command::new("node")
        .arg("--version")
        .output()
        .await
        .map(|o| o.status.success())
        .unwrap_or(false)
}

async fn resolve_bundler(root: &Path) -> Result<PathBuf, BuildError> {
    let output = tokio::process::Command::new("node")
        .arg("-p")
        .arg("require.resolve('usai/build/bundle.mjs', { paths: [process.argv[1]] })")
        .arg(root)
        .output()
        .await?;
    if !output.status.success() {
        return Err(BuildError::SdkMissing {
            root: root.to_path_buf(),
            detail: String::from_utf8_lossy(&output.stderr)
                .lines()
                .last()
                .unwrap_or("")
                .to_owned(),
        });
    }
    Ok(PathBuf::from(
        String::from_utf8_lossy(&output.stdout).trim(),
    ))
}

async fn bundle(root: &Path, entry: &Path, outfile: &Path) -> Result<Vec<PathBuf>, BuildError> {
    if !node_available().await {
        return Err(BuildError::NodeMissing);
    }
    let bundler = resolve_bundler(root).await?;
    if let Some(parent) = outfile.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let output = tokio::process::Command::new("node")
        .arg(&bundler)
        .arg(entry)
        .arg(outfile)
        .current_dir(root)
        .output()
        .await?;
    let report: BundleReport = serde_json::from_slice(&output.stdout).map_err(|_| {
        BuildError::Bundle(format!(
            "bundler produced no report\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ))
    })?;
    if !report.ok {
        let lines: Vec<String> = report
            .errors
            .iter()
            .map(|e| match &e.location {
                Some(l) => format!("  {}:{}:{}: {}", l.file, l.line, l.column, e.text),
                None => format!("  {}", e.text),
            })
            .collect();
        return Err(BuildError::Bundle(lines.join("\n")));
    }
    Ok(report
        .inputs
        .into_iter()
        .map(|p| {
            let path = PathBuf::from(p);
            if path.is_absolute() {
                path
            } else {
                root.join(path)
            }
        })
        .filter(|p| !p.components().any(|c| c.as_os_str() == "node_modules"))
        .collect())
}

/// One configuration value with where it came from (`GOAL.md` §29).
#[derive(Clone, Debug, serde::Serialize, PartialEq, Eq)]
pub struct Sourced<T> {
    pub value: T,
    pub source: &'static str,
}

/// The effective project configuration. Maps structure; never semantics.
#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectConfig {
    pub root: PathBuf,
    pub config_file: Option<PathBuf>,
    pub app: Sourced<PathBuf>,
    pub out_dir: Sourced<PathBuf>,
    pub migrations: Sourced<Vec<String>>,
    pub seeders: Sourced<Vec<String>>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct RawConfig {
    app: Option<String>,
    out_dir: Option<String>,
    database: Option<RawDatabase>,
}

#[derive(Deserialize, Default)]
struct RawDatabase {
    migrations: Option<RawInclude>,
    seeders: Option<RawInclude>,
}

#[derive(Deserialize, Default)]
struct RawInclude {
    include: Vec<String>,
}

const CONFIG_FILE: &str = "usai.config.ts";

/// Resolves `usai.config.ts` (if present) into the effective configuration.
/// The file is bundled and evaluated in a capability-less world, so it can
/// use TypeScript syntax but cannot perform I/O (contract C9).
pub async fn load_config(engine: &dyn Engine, root: &Path) -> Result<ProjectConfig, BuildError> {
    let config_path = root.join(CONFIG_FILE);
    let (raw, source): (RawConfig, &'static str) = if config_path.exists() {
        let outfile = root.join(".usai/config/usai.config.js");
        bundle(root, &config_path, &outfile).await?;
        let code = Code::new(tokio::fs::read_to_string(&outfile).await?);
        let compiled = engine.compile_code(&code).await?;
        let value = engine.export_default(&compiled).await?;
        (serde_json::from_value(value)?, CONFIG_FILE)
    } else {
        (RawConfig::default(), "default")
    };
    let pick = |value: Option<String>, default: &str| -> Sourced<PathBuf> {
        match value {
            Some(v) => Sourced {
                value: root.join(v),
                source,
            },
            None => Sourced {
                value: root.join(default),
                source: "default",
            },
        }
    };
    let db = raw.database.unwrap_or_default();
    let include = |value: Option<RawInclude>, default: Vec<String>| -> Sourced<Vec<String>> {
        match value {
            Some(v) => Sourced {
                value: v.include,
                source,
            },
            None => Sourced {
                value: default,
                source: "default",
            },
        }
    };
    Ok(ProjectConfig {
        root: root.to_path_buf(),
        config_file: config_path.exists().then_some(config_path),
        app: pick(raw.app, "src/app.ts"),
        out_dir: pick(raw.out_dir, ".usai/build"),
        migrations: include(
            db.migrations,
            vec![
                "./src/**/migrations/*.sql".into(),
                "./migrations/*.sql".into(),
            ],
        ),
        seeders: include(
            db.seeders,
            vec!["./src/**/seeders/*.ts".into(), "./seeders/*.ts".into()],
        ),
    })
}

impl BuildOptions {
    pub fn from_config(config: &ProjectConfig) -> Self {
        Self {
            root: config.root.clone(),
            entry: config.app.value.clone(),
            out_dir: config.out_dir.value.clone(),
        }
    }
}

pub async fn build(engine: &dyn Engine, options: &BuildOptions) -> Result<BuildOutput, BuildError> {
    let code_path = options.out_dir.join("app.js");
    let manifest_path = options.out_dir.join("manifest.json");
    let entry = if options.entry.is_absolute() {
        options.entry.clone()
    } else {
        options.root.join(&options.entry)
    };
    let mut inputs = bundle(&options.root, &entry, &code_path).await?;
    let config_path = options.root.join(CONFIG_FILE);
    if config_path.exists() {
        inputs.push(config_path);
    }

    let source = tokio::fs::read_to_string(&code_path).await?;
    let code = Code::new(source);
    let compiled = engine.compile_code(&code).await?;
    let mut manifest_value = engine.describe(&compiled).await?;
    manifest_value["codeSha256"] = serde_json::Value::String(code.sha256.clone());
    let manifest: Manifest = serde_json::from_value(manifest_value)?;
    tokio::fs::write(&manifest_path, serde_json::to_vec_pretty(&manifest)?).await?;
    let definition = ApplicationDefinition::new(manifest, code)?;
    Ok(BuildOutput {
        definition,
        manifest_path,
        code_path,
        inputs,
    })
}

/// Loads a previously built artifact directory.
pub async fn load_artifact(dir: &Path) -> Result<Arc<ApplicationDefinition>, BuildError> {
    let manifest: Manifest =
        serde_json::from_slice(&tokio::fs::read(dir.join("manifest.json")).await?)?;
    let code = Code::new(tokio::fs::read_to_string(dir.join("app.js")).await?);
    Ok(ApplicationDefinition::new(manifest, code)?)
}
