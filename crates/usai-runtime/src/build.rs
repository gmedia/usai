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
    // The engine's compiled form next to the artifact, so installing it is
    // a load rather than a compile (which takes every core for seconds).
    let image_path = options.out_dir.join(IMAGE_FILE);
    let meta_path = options.out_dir.join(IMAGE_META_FILE);
    let _ = tokio::fs::remove_file(&image_path).await;
    let _ = tokio::fs::remove_file(&meta_path).await;
    if let Some(bytes) = engine.precompile(&compiled) {
        tokio::fs::create_dir_all(image_path.parent().expect("cache dir")).await?;
        let meta = ImageMeta {
            engine: engine.name().to_owned(),
            fingerprint: engine.fingerprint(),
            code_sha256: code.sha256.clone(),
            sha256: {
                use sha2::Digest as _;
                hex::encode(sha2::Sha256::digest(&bytes))
            },
        };
        tokio::fs::write(&image_path, &bytes).await?;
        tokio::fs::write(&meta_path, serde_json::to_vec_pretty(&meta)?).await?;
    }
    let definition = ApplicationDefinition::new(manifest, code)?;
    Ok(BuildOutput {
        definition,
        manifest_path,
        code_path,
        inputs,
    })
}

/// The engine cache lives apart from the logical artifact (`manifest.json`
/// and `app.js`): a deployment may ship or drop the `cache/` directory
/// without changing what the artifact is (ADR-0005).
const IMAGE_FILE: &str = "cache/image.cwasm";
const IMAGE_META_FILE: &str = "cache/image.json";

/// What `image.cwasm` was built from and with. Any mismatch at load time
/// means the file is ignored and the engine compiles from `app.js`.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ImageMeta {
    engine: String,
    fingerprint: String,
    code_sha256: String,
    sha256: String,
}

/// Loads a previously built artifact directory. A precompiled image is
/// attached when it was built from exactly this code and its digest holds
/// (`USAI_PRECOMPILED=0` ignores it).
pub async fn load_artifact(dir: &Path) -> Result<Arc<ApplicationDefinition>, BuildError> {
    let manifest: Manifest =
        serde_json::from_slice(&tokio::fs::read(dir.join("manifest.json")).await?)?;
    let code = Code::new(tokio::fs::read_to_string(dir.join("app.js")).await?);
    let definition = ApplicationDefinition::new(manifest, code)?;
    if std::env::var("USAI_PRECOMPILED").as_deref() == Ok("0") {
        return Ok(definition);
    }
    let Ok(meta) = tokio::fs::read(dir.join(IMAGE_META_FILE)).await else {
        return Ok(definition);
    };
    let Ok(meta) = serde_json::from_slice::<ImageMeta>(&meta) else {
        tracing::warn!("{IMAGE_META_FILE} is not readable; ignoring the precompiled image");
        return Ok(definition);
    };
    if meta.code_sha256 != definition.code().sha256 {
        tracing::warn!("{IMAGE_FILE} was built from other code; ignoring it");
        return Ok(definition);
    }
    let Ok(bytes) = tokio::fs::read(dir.join(IMAGE_FILE)).await else {
        return Ok(definition);
    };
    let digest = {
        use sha2::Digest as _;
        hex::encode(sha2::Sha256::digest(&bytes))
    };
    if digest != meta.sha256 {
        tracing::warn!("{IMAGE_FILE} does not match {IMAGE_META_FILE}; ignoring it");
        return Ok(definition);
    }
    Ok(definition.with_precompiled(crate::definition::Precompiled {
        engine: meta.engine,
        fingerprint: meta.fingerprint,
        bytes: bytes.into(),
    }))
}

/// Builds a definition whose only workload is `command:seed:<name>` running
/// the seeder file's default export with the application's resources and
/// env (`GOAL.md` §34). Seeders are finite application work, never part of
/// the application's own definition.
pub async fn build_seeder(
    engine: &dyn Engine,
    options: &BuildOptions,
    seeder_path: &Path,
    name: &str,
) -> Result<BuildOutput, BuildError> {
    let entry_dir = options.root.join(".usai/seed");
    tokio::fs::create_dir_all(&entry_dir).await?;
    let app_entry = if options.entry.is_absolute() {
        options.entry.clone()
    } else {
        options.root.join(&options.entry)
    };
    let rel = |p: &Path| -> String {
        let rel = pathdiff(&entry_dir, p);
        rel.to_string_lossy().replace('\\', "/")
    };
    let entry_source = format!(
        "import app from {app:?};\nimport seed from {seed:?};\nimport {{ defineApp, command }} from \"usai\";\n\
         const run = command({name:?}, {{ resources: [...(seed.resources ?? [])] }}, async (ctx) => seed.run(ctx));\n\
         export default defineApp({{ name: app.name + \":seed\", workloads: [run], resources: [...app.resources, ...app.modules.flatMap((m) => m.resources)], ...(app.env ? {{ env: app.env }} : {{}}) }});\n",
        app = rel(&app_entry),
        seed = rel(seeder_path),
        name = format!("seed:{name}"),
    );
    let entry_path = entry_dir.join(format!("{name}.entry.ts"));
    tokio::fs::write(&entry_path, entry_source).await?;
    let seed_options = BuildOptions {
        root: options.root.clone(),
        entry: entry_path,
        out_dir: entry_dir.join(name),
    };
    build(engine, &seed_options).await
}

/// Relative path from `from` (a directory) to `to`, with `./` prefix.
fn pathdiff(from: &Path, to: &Path) -> PathBuf {
    let from: Vec<_> = from.components().collect();
    let to_components: Vec<_> = to.components().collect();
    let common = from
        .iter()
        .zip(&to_components)
        .take_while(|(a, b)| a == b)
        .count();
    let mut out = PathBuf::new();
    if from.len() == common {
        out.push(".");
    }
    for _ in common..from.len() {
        out.push("..");
    }
    for c in &to_components[common..] {
        out.push(c);
    }
    out
}
