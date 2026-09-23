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
    /// Project root (where `node_modules/@sakaladev/usai` resolves from).
    pub root: PathBuf,
    /// Application entry, relative to `root` or absolute.
    pub entry: PathBuf,
    /// Output directory for `app.js` and `manifest.json`.
    pub out_dir: PathBuf,
    /// Root-relative migration globs from the project config (module globs
    /// come from the definition). The matching files are copied into the
    /// artifact so `usai db migrate --artifact <dir>` works where there is
    /// no source tree (a production image).
    pub migration_globs: Vec<String>,
}

impl BuildOptions {
    pub fn for_project(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        Self {
            entry: root.join("src/app.ts"),
            out_dir: root.join(".usai/build"),
            root,
            migration_globs: Vec::new(),
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
        "could not resolve the `@sakaladev/usai` package from {root}: {detail}\n  hint: run `pnpm add @sakaladev/usai` (or npm install) in the project"
    )]
    SdkMissing { root: PathBuf, detail: String },
    #[error(
        "{root} is not a Usai project: no package.json or usai.config.ts here\n  hint: run this inside the project directory, or pass --root <dir>; `pnpm dlx @sakaladev/create-usai <dir>` creates one"
    )]
    NotAProject { root: PathBuf },
    #[error(
        "{root} is the scaffold template, not a project: `package.json` still has the placeholders `create-usai` fills in (`__NAME__`, `__USAI_VERSION__`).\n  hint: `pnpm dlx @sakaladev/create-usai <dir>` writes a real project; copying `templates/hello` by hand leaves the placeholders behind"
    )]
    UnfilledTemplate { root: PathBuf },
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
    #[error(
        "usai.config.ts could not be evaluated: {detail}\n  \
         usai.config.ts is a declaration, evaluated in a capability-less world — not a Node script: \
         `process.env`, `fs`, `require`, `import.meta` are not available there and the result is \
         cached by its source digest. Keep it to structure (app entry, migrations, seeders); per-environment \
         values belong to `env(...)` declarations in the application and to the runtime's environment (§13 of docs/GUIDE.md)."
    )]
    ConfigNotDeclarative { detail: String },
    #[error("artifact refused: {0}")]
    Signature(String),
}

/// What esbuild's own words do not say. A bundle failure reaching a user is
/// almost always one of two rules they have not met yet, and both are the
/// application model rather than the bundler: a module that reaches for Node
/// at load time cannot be part of an application, and an application module
/// is evaluated to be read, not to run work.
fn bundle_hint(errors: &[BundleMessage]) -> String {
    const BUILTINS: [&str; 23] = [
        "fs",
        "path",
        "os",
        "net",
        "tls",
        "dns",
        "http",
        "https",
        "events",
        "stream",
        "util",
        "assert",
        "crypto",
        "zlib",
        "buffer",
        "url",
        "tty",
        "child_process",
        "cluster",
        "worker_threads",
        "perf_hooks",
        "readline",
        "module",
    ];
    let mut hints = Vec::new();
    // esbuild says: Could not resolve "node:fs"
    let specifier = |text: &str| -> Option<String> {
        let rest = text.strip_prefix("Could not resolve \"")?;
        rest.split('"').next().map(str::to_owned)
    };
    let unresolved: Vec<&BundleMessage> = errors
        .iter()
        .filter(|e| {
            specifier(&e.text).is_some_and(|spec| {
                spec.starts_with("node:")
                    || BUILTINS
                        .iter()
                        .any(|b| spec == *b || spec.starts_with(&format!("{b}/")))
            })
        })
        .collect();
    if !unresolved.is_empty() {
        let mut packages: Vec<String> = Vec::new();
        for location in unresolved.iter().filter_map(|e| e.location.as_ref()) {
            let Some(after) = location.file.split("node_modules/").nth(1) else {
                continue;
            };
            let mut parts = after.split('/');
            let name = match parts.next() {
                // A scoped package is two segments.
                Some(scope) if scope.starts_with('@') => {
                    format!("{scope}/{}", parts.next().unwrap_or_default())
                }
                Some(name) => name.to_owned(),
                None => continue,
            };
            if !name.is_empty() && !packages.contains(&name) {
                packages.push(name);
            }
        }
        hints.push(match packages.len() {
            0 => "\n\n  A world has no filesystem, no process and no Node built-ins: an application \
                 module cannot import one (docs/GUIDE.md §12)."
                .to_owned(),
            _ => format!(
                "\n\n  A world has no Node. {} — and anything that depends on {} — reach for a Node \
                 built-in module when they are loaded, so they cannot be part of an application. A \
                 library that only computes can (a query builder used to *compile* SQL, a validator, \
                 a date library); one that opens a socket, a file or a connection of its own cannot: \
                 the database, outbound HTTP and time are the runtime's to lease (docs/GUIDE.md §7, \
                 §11, §19).",
                packages
                    .iter()
                    .take(3)
                    .map(|p| format!("`{p}`"))
                    .collect::<Vec<_>>()
                    .join(", "),
                if packages.len() > 3 { "them" } else { "it" },
            ),
        });
        if unresolved.iter().any(|e| {
            e.location
                .as_ref()
                .is_none_or(|l| !l.file.contains("node_modules"))
        }) {
            hints.push(
                "\n\n  The runtime never scans your files either: every workload reaches the \
                 application through an `import`, and `defineApp({ workloads, modules })` is the \
                 list (docs/GUIDE.md §3). Discovering route files with `fs`, a glob or a dynamic \
                 `import()` cannot work."
                    .to_owned(),
            );
        }
    }
    if errors.iter().any(|e| e.text.contains("Top-level await")) {
        hints.push(
            "\n\n  Top-level `await` is not available in an application module: the module is \
             evaluated to read what the application declares, in a world with no capabilities, \
             before any work runs. Move the await inside a workload handler (docs/GUIDE.md §3)."
                .to_owned(),
        );
    }
    hints.join("")
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
        .arg("require.resolve('@sakaladev/usai/build/bundle.mjs', { paths: [process.argv[1]] })")
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
        return Err(BuildError::Bundle(format!(
            "{}{}",
            lines.join("\n"),
            bundle_hint(&report.errors)
        )));
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
    let package_json = root.join("package.json");
    if !config_path.exists() && !package_json.exists() {
        return Err(BuildError::NotAProject {
            root: root.to_path_buf(),
        });
    }
    // A hand-copied `templates/hello` fails four steps later, on an npm
    // install of a package named `__USAI_VERSION__`. Say it here instead.
    if let Ok(text) = std::fs::read_to_string(&package_json)
        && (text.contains("__USAI_VERSION__") || text.contains("__NAME__"))
    {
        return Err(BuildError::UnfilledTemplate {
            root: root.to_path_buf(),
        });
    }
    let (raw, source): (RawConfig, &'static str) = if config_path.exists() {
        let outfile = root.join(".usai/config/usai.config.js");
        bundle(root, &config_path, &outfile).await?;
        let code = Code::new(tokio::fs::read_to_string(&outfile).await?);
        // Evaluating the config means building an image (hundreds of ms);
        // the result only depends on the bundled source, so cache it by its
        // digest next to the bundle.
        let cached = root.join(".usai/config/usai.config.json");
        let value = match tokio::fs::read(&cached)
            .await
            .ok()
            .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
        {
            Some(v)
                if v.get("sha256").and_then(serde_json::Value::as_str)
                    == Some(code.sha256.as_str()) =>
            {
                v.get("config").cloned().unwrap_or(serde_json::Value::Null)
            }
            _ => {
                let not_declarative = |e: &EngineError| {
                    let text = e.to_string();
                    [
                        "process is not defined",
                        "require is not defined",
                        "import.meta",
                        "fs is not defined",
                        "__dirname",
                    ]
                    .iter()
                    .any(|needle| text.contains(needle))
                    .then_some(BuildError::ConfigNotDeclarative { detail: text })
                };
                let compiled = match engine.compile_code(&code).await {
                    Ok(c) => c,
                    Err(e) => return Err(not_declarative(&e).unwrap_or(BuildError::Engine(e))),
                };
                let value = match engine.export_default(&compiled).await {
                    Ok(v) => v,
                    Err(e) => return Err(not_declarative(&e).unwrap_or(BuildError::Engine(e))),
                };
                let _ = tokio::fs::write(
                    &cached,
                    serde_json::to_vec(
                        &serde_json::json!({ "sha256": code.sha256, "config": value }),
                    )
                    .unwrap_or_default(),
                )
                .await;
                value
            }
        };
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
            migration_globs: config.migrations.value.clone(),
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
    // Provenance for compatibility diagnostics: the SDK version comes from
    // the bundle itself (`describe()` stamps it), the runtime's from here.
    let sdk = manifest_value
        .get("builtWith")
        .and_then(|b| b.get("sdk"))
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    // The guest ABI the bundled SDK speaks: what the runtime checks at
    // install (`GUEST_ABI`); a bundle that predates the stamp spoke ABI 1.
    let abi = manifest_value
        .get("builtWith")
        .and_then(|b| b.get("abi"))
        .cloned()
        .unwrap_or(serde_json::json!(1));
    manifest_value["builtWith"] = serde_json::json!({
        "sdk": sdk,
        "runtime": crate::definition::RUNTIME_VERSION,
        "abi": abi,
    });
    let manifest: Manifest = serde_json::from_value(manifest_value)?;
    tokio::fs::write(&manifest_path, serde_json::to_vec_pretty(&manifest)?).await?;
    // The engine's compiled form next to the artifact, so installing it is
    // a load rather than a compile (which takes every core for seconds).
    let image_path = options.out_dir.join(IMAGE_FILE);
    let meta_path = options.out_dir.join(IMAGE_META_FILE);
    let _ = tokio::fs::remove_file(&image_path).await;
    let _ = tokio::fs::remove_file(&meta_path).await;
    // A signature from a previous build never describes this one.
    let _ = tokio::fs::remove_file(options.out_dir.join(crate::signing::SIGNATURE_FILE)).await;
    let mut precompiled = None;
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
        precompiled = Some(crate::definition::Precompiled {
            engine: meta.engine,
            fingerprint: meta.fingerprint,
            bytes: bytes.into(),
        });
    }
    // Migrations travel with the artifact (`migrations/<name>`), so a
    // production image can apply them without the source tree.
    let migrations_dir = options.out_dir.join(MIGRATIONS_DIR);
    let _ = tokio::fs::remove_dir_all(&migrations_dir).await;
    let globs = crate::db::migration_globs_for(&manifest.modules, &options.migration_globs);
    if !globs.is_empty() {
        let files = crate::db::discover_migrations(&options.root, &globs)
            .map_err(|e| BuildError::Bundle(format!("migrations: {e}")))?;
        if !files.is_empty() {
            tokio::fs::create_dir_all(&migrations_dir).await?;
            for file in &files {
                tokio::fs::copy(&file.path, migrations_dir.join(&file.name)).await?;
                inputs.push(file.path.clone());
            }
        }
    }
    // The source files this artifact was built from, so tools that reuse the
    // artifact can tell when it is stale (`artifact_is_current`).
    let _ = tokio::fs::write(
        options.out_dir.join(INPUTS_FILE),
        serde_json::to_vec_pretty(
            &inputs
                .iter()
                .map(|p| p.to_string_lossy())
                .collect::<Vec<_>>(),
        )?,
    )
    .await;
    // The definition the caller installs carries the compiled form too, so
    // `usai dev` loads what the build just compiled instead of compiling a
    // second time on every core.
    let definition = ApplicationDefinition::new(manifest, code)?;
    let definition = match precompiled {
        Some(pre) => definition.with_precompiled(pre),
        None => definition,
    };
    let definition = match read_source_map(&code_path).await {
        Some(map) => definition.with_source_map(map),
        None => definition,
    };
    Ok(BuildOutput {
        definition,
        manifest_path,
        code_path,
        inputs,
    })
}

const INPUTS_FILE: &str = "inputs.json";
/// Where an artifact carries its migrations.
pub const MIGRATIONS_DIR: &str = "migrations";

/// `app.js.map` next to the bundle, when the bundler wrote one.
async fn read_source_map(code_path: &Path) -> Option<Arc<crate::sourcemap::SourceMap>> {
    let mut map_path = code_path.as_os_str().to_owned();
    map_path.push(".map");
    let text = tokio::fs::read_to_string(std::path::PathBuf::from(map_path))
        .await
        .ok()?;
    crate::sourcemap::SourceMap::parse(&text)
}

/// Whether the artifact in `dir` is at least as new as every source file it
/// was built from. `false` when the input list is missing (an artifact
/// produced elsewhere), so callers rebuild rather than trust it.
pub fn artifact_is_current(dir: &Path) -> bool {
    let Ok(built) = std::fs::metadata(dir.join("manifest.json")).and_then(|m| m.modified()) else {
        return false;
    };
    let Ok(inputs) = std::fs::read(dir.join(INPUTS_FILE)) else {
        return false;
    };
    let Ok(inputs) = serde_json::from_slice::<Vec<String>>(&inputs) else {
        return false;
    };
    inputs.iter().all(|input| {
        std::fs::metadata(input)
            .and_then(|m| m.modified())
            .map(|changed| changed <= built)
            .unwrap_or(false)
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
/// `load_artifact`, refusing first when the runtime requires a signature
/// and the artifact's does not verify against `trusted`.
pub async fn load_artifact_trusted(
    dir: &Path,
    trusted: &[ed25519_dalek::VerifyingKey],
) -> Result<Arc<ApplicationDefinition>, BuildError> {
    if !trusted.is_empty() {
        crate::signing::verify_artifact(dir, trusted)
            .map_err(|e| BuildError::Signature(e.to_string()))?;
    }
    load_artifact(dir).await
}

pub async fn load_artifact(dir: &Path) -> Result<Arc<ApplicationDefinition>, BuildError> {
    let manifest: Manifest =
        serde_json::from_slice(&tokio::fs::read(dir.join("manifest.json")).await?)?;
    let code = Code::new(tokio::fs::read_to_string(dir.join("app.js")).await?);
    let definition = ApplicationDefinition::new(manifest, code)?;
    let definition = match read_source_map(&dir.join("app.js")).await {
        Some(map) => definition.with_source_map(map),
        None => definition,
    };
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
        "import app from {app:?};\nimport seed from {seed:?};\nimport {{ defineApp, command }} from \"@sakaladev/usai\";\n\
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
        // A seeder's throwaway build does not carry migrations.
        migration_globs: Vec::new(),
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
