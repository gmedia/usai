//! Project database work: migration discovery and application (`GOAL.md`
//! §33). Migrations are finite work under runtime ownership — each one runs
//! on a leased connection in its own transaction and is recorded in the
//! `usai_migrations` ledger in the same transaction. They never run
//! implicitly at startup.

use std::path::{Path, PathBuf};

use serde::Serialize;
use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken;

use crate::definition::ApplicationDefinition;
use crate::resource::ResourceError;
use crate::resource::postgres::{AppliedMigration, Postgres};
use crate::runtime::Revision;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrationFile {
    pub name: String,
    pub path: PathBuf,
    pub checksum: String,
    /// Which glob (config or module) discovered it.
    pub source: String,
}

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("invalid glob {0}: {1}")]
    Glob(String, String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("duplicate migration name {name}: {a} and {b}")]
    Duplicate {
        name: String,
        a: PathBuf,
        b: PathBuf,
    },
    #[error("the application declares no postgres resource")]
    NoDatabase,
    #[error("resource {0} is not a postgres resource")]
    NotPostgres(String),
    #[error(
        "migration {name} was applied with checksum {applied} but the file now hashes to {current}; migrations are immutable once applied"
    )]
    ChecksumMismatch {
        name: String,
        applied: String,
        current: String,
    },
    #[error(transparent)]
    Resource(#[from] ResourceError),
}

/// Discovers migration files from root-relative globs. Names are file
/// basenames; ordering is by name, then path, so `001_users.sql` runs
/// before `002_orders.sql` regardless of which directory declares it.
pub fn discover_migrations(
    root: &Path,
    globs: &[(String, String)],
) -> Result<Vec<MigrationFile>, DbError> {
    let mut files: Vec<MigrationFile> = Vec::new();
    for (pattern, source) in globs {
        let absolute = root.join(pattern.trim_start_matches("./"));
        let pattern_str = absolute.to_string_lossy().into_owned();
        let paths =
            glob::glob(&pattern_str).map_err(|e| DbError::Glob(pattern.clone(), e.to_string()))?;
        for path in paths.flatten() {
            if !path.is_file() {
                continue;
            }
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            if files.iter().any(|f| f.path == path) {
                continue;
            }
            let contents = std::fs::read(&path)?;
            files.push(MigrationFile {
                name,
                checksum: hex::encode(Sha256::digest(&contents))[..16].to_owned(),
                path,
                source: source.clone(),
            });
        }
    }
    files.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.path.cmp(&b.path)));
    for pair in files.windows(2) {
        if pair[0].name == pair[1].name {
            return Err(DbError::Duplicate {
                name: pair[0].name.clone(),
                a: pair[0].path.clone(),
                b: pair[1].path.clone(),
            });
        }
    }
    Ok(files)
}

/// All migration globs for a definition: config includes plus module
/// declarations (both root-relative).
pub fn migration_globs(
    definition: &ApplicationDefinition,
    config_globs: &[String],
) -> Vec<(String, String)> {
    migration_globs_for(&definition.manifest().modules, config_globs)
}

pub fn migration_globs_for(
    modules: &[crate::definition::ModuleSpec],
    config_globs: &[String],
) -> Vec<(String, String)> {
    let mut globs: Vec<(String, String)> = config_globs
        .iter()
        .map(|g| (g.clone(), "usai.config.ts".to_owned()))
        .collect();
    for module in modules {
        for g in &module.migrations {
            globs.push((g.clone(), format!("module {}", module.name)));
        }
    }
    globs
}

/// The migrations an artifact carries (`<artifact>/migrations/*.sql`).
pub fn artifact_migrations(artifact: &Path) -> Result<Vec<MigrationFile>, DbError> {
    discover_migrations(
        artifact,
        &[(
            format!("{}/*.sql", crate::build::MIGRATIONS_DIR),
            "artifact".to_owned(),
        )],
    )
}

pub fn seeder_globs(
    definition: &ApplicationDefinition,
    config_globs: &[String],
) -> Vec<(String, String)> {
    let mut globs: Vec<(String, String)> = config_globs
        .iter()
        .map(|g| (g.clone(), "usai.config.ts".to_owned()))
        .collect();
    for module in &definition.manifest().modules {
        for g in &module.seeders {
            globs.push((g.clone(), format!("module {}", module.name)));
        }
    }
    globs
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SeederFile {
    /// `dev` for `src/users/seeders/dev.ts`.
    pub name: String,
    pub path: PathBuf,
    pub source: String,
}

pub fn discover_seeders(
    root: &Path,
    globs: &[(String, String)],
) -> Result<Vec<SeederFile>, DbError> {
    let mut files: Vec<SeederFile> = Vec::new();
    for (pattern, source) in globs {
        let absolute = root.join(pattern.trim_start_matches("./"));
        let paths = glob::glob(&absolute.to_string_lossy())
            .map_err(|e| DbError::Glob(pattern.clone(), e.to_string()))?;
        for path in paths.flatten() {
            if !path.is_file() || files.iter().any(|f| f.path == path) {
                continue;
            }
            let name = path
                .file_stem()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            files.push(SeederFile {
                name,
                path,
                source: source.clone(),
            });
        }
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(files)
}

/// The postgres manager migrations target: the named resource, or the
/// first postgres resource the definition declares.
pub fn database(
    revision: &Revision,
    name: Option<&str>,
) -> Result<std::sync::Arc<dyn crate::resource::ResourceManager>, DbError> {
    let resources = revision.resources();
    let name = match name {
        Some(n) => n.to_owned(),
        None => revision
            .definition
            .resources()
            .iter()
            .find(|r| r.kind == "postgres")
            .map(|r| r.name.clone())
            .ok_or(DbError::NoDatabase)?,
    };
    let manager = resources
        .get(&name)
        .cloned()
        .ok_or_else(|| DbError::NotPostgres(name.clone()))?;
    if manager.as_any().downcast_ref::<Postgres>().is_none() {
        return Err(DbError::NotPostgres(name));
    }
    Ok(manager)
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrationStatus {
    pub name: String,
    pub checksum: String,
    pub applied_at: Option<String>,
    pub path: Option<PathBuf>,
}

pub async fn status(
    manager: &dyn crate::resource::ResourceManager,
    files: &[MigrationFile],
) -> Result<Vec<MigrationStatus>, DbError> {
    let pg = manager
        .as_any()
        .downcast_ref::<Postgres>()
        .ok_or(DbError::NoDatabase)?;
    pg.ensure_migration_ledger(CancellationToken::new()).await?;
    let applied: Vec<AppliedMigration> = pg.applied_migrations(CancellationToken::new()).await?;
    let mut out: Vec<MigrationStatus> = files
        .iter()
        .map(|f| MigrationStatus {
            name: f.name.clone(),
            checksum: f.checksum.clone(),
            applied_at: applied
                .iter()
                .find(|a| a.name == f.name)
                .map(|a| a.applied_at.clone()),
            path: Some(f.path.clone()),
        })
        .collect();
    for a in applied {
        if !files.iter().any(|f| f.name == a.name) {
            out.push(MigrationStatus {
                name: a.name,
                checksum: a.checksum,
                applied_at: Some(a.applied_at),
                path: None,
            });
        }
    }
    Ok(out)
}

/// Applies every pending migration in order. Stops at the first failure;
/// already-applied migrations with a different checksum are refused.
pub async fn migrate(
    manager: &dyn crate::resource::ResourceManager,
    files: &[MigrationFile],
    cancel: CancellationToken,
) -> Result<Vec<String>, DbError> {
    let pg = manager
        .as_any()
        .downcast_ref::<Postgres>()
        .ok_or(DbError::NoDatabase)?;
    pg.ensure_migration_ledger(cancel.clone()).await?;
    let applied = pg.applied_migrations(cancel.clone()).await?;
    let mut done = Vec::new();
    for file in files {
        if let Some(existing) = applied.iter().find(|a| a.name == file.name) {
            if existing.checksum != file.checksum {
                return Err(DbError::ChecksumMismatch {
                    name: file.name.clone(),
                    applied: existing.checksum.clone(),
                    current: file.checksum.clone(),
                });
            }
            continue;
        }
        let sql = tokio::fs::read_to_string(&file.path).await?;
        tracing::info!(migration = %file.name, "applying");
        if pg
            .apply_migration(&file.name, &sql, &file.checksum, cancel.clone())
            .await?
        {
            done.push(file.name.clone());
        } else {
            tracing::info!(migration = %file.name, "already applied by another migrator");
        }
    }
    Ok(done)
}
