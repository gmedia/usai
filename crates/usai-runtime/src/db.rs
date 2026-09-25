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

/// A module's glob is tried twice — as written, then relative to the module
/// file — so the warning belongs to the pair, not to either attempt. The
/// config's globs are allowed to match nothing (a project without
/// migrations); a module's are named, because a typo there is a silent
/// half-deployment.
///
/// The fallback candidate is marked in the source string so a miss on it
/// alone stays quiet.
const FALLBACK: &str = " (module-relative)";

fn warn_if_empty(what: &str, attempts: &[(String, String, usize)]) {
    for (pattern, source, _) in attempts {
        if !source.starts_with("module ") || source.ends_with(FALLBACK) {
            continue;
        }
        // Did anything this module declared match, either way round?
        let module_matched = attempts
            .iter()
            .any(|(_, s, n)| *n > 0 && s.trim_end_matches(FALLBACK) == source);
        if !module_matched {
            tracing::warn!(
                "{source} declares {what} {pattern:?}, which matches no file — neither as written (from the project root) nor next to the module itself"
            );
        }
    }
}

/// Discovers migration files from root-relative globs. Names are file
/// basenames; ordering is by name, then path, so `001_users.sql` runs
/// before `002_orders.sql` regardless of which directory declares it.
pub fn discover_migrations(
    root: &Path,
    globs: &[(String, String)],
) -> Result<Vec<MigrationFile>, DbError> {
    let mut files: Vec<MigrationFile> = Vec::new();
    let mut per_source: Vec<(String, String, usize)> = Vec::new();
    for (pattern, source) in globs {
        let absolute = root.join(pattern.trim_start_matches("./"));
        let pattern_str = absolute.to_string_lossy().into_owned();
        let paths =
            glob::glob(&pattern_str).map_err(|e| DbError::Glob(pattern.clone(), e.to_string()))?;
        let mut matched = 0usize;
        for path in paths.flatten() {
            if !path.is_file() {
                continue;
            }
            matched += 1;
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
        per_source.push((pattern.clone(), source.clone(), matched));
    }
    warn_if_empty("migrations", &per_source);
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
            // A module may write its globs relative to itself. The glob as
            // written wins when it matches (every application built before
            // this keeps working); this is the fallback, and `discover_*`
            // skips a duplicate file.
            if let Some(relative) = module_relative(module, g) {
                globs.push((relative, format!("module {}{FALLBACK}", module.name)));
            }
        }
    }
    globs
}

/// `./migrations/*.sql` in `src/billing/module.ts` → `src/billing/migrations/*.sql`.
fn module_relative(module: &crate::definition::ModuleSpec, glob: &str) -> Option<String> {
    let dir = module.source_dir.as_deref()?;
    if dir.is_empty() {
        return None;
    }
    let trimmed = glob.trim_start_matches("./");
    // An absolute-looking or already-prefixed glob is left alone.
    if trimmed.starts_with('/') || trimmed.starts_with(dir) {
        return None;
    }
    Some(format!("{dir}/{trimmed}"))
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
            if let Some(relative) = module_relative(module, g) {
                globs.push((relative, format!("module {}{FALLBACK}", module.name)));
            }
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
    let mut per_source: Vec<(String, String, usize)> = Vec::new();
    for (pattern, source) in globs {
        let absolute = root.join(pattern.trim_start_matches("./"));
        let paths = glob::glob(&absolute.to_string_lossy())
            .map_err(|e| DbError::Glob(pattern.clone(), e.to_string()))?;
        let mut matched = 0usize;
        for path in paths.flatten() {
            if !path.is_file() {
                continue;
            }
            matched += 1;
            if files.iter().any(|f| f.path == path) {
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
        per_source.push((pattern.clone(), source.clone(), matched));
    }
    warn_if_empty("seeders", &per_source);
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
    /// Which glob found this file — `usai.config.ts` or `module <name>`.
    ///
    /// Migrations are **one history for one database**, ordered by file name
    /// across every module, because a module is organisation and not
    /// isolation: a foreign key from one module's table to another's is
    /// ordinary, and two histories could not express it. That makes the
    /// numbering a project-wide decision, and this column is what makes the
    /// resulting order legible — without it, "why does this run before that"
    /// has no answer on the screen.
    #[serde(default)]
    pub source: String,
    /// The file's checksum as it is on disk now.
    pub checksum: String,
    pub applied_at: Option<String>,
    /// The checksum recorded in the ledger when this migration was applied.
    /// It is the *other* number: `checksum` above is what the file hashes to
    /// today, and printing only that made a file edited after it was applied
    /// indistinguishable from an untouched one — in the table and in
    /// `--json` — while `usai db migrate` refuses the same state outright.
    /// `db status` is the command the restore runbook designates as the
    /// gate ("everything applied and nothing pending is the only state to
    /// start from"), so it has to be able to see drift.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub applied_checksum: Option<String>,
    pub path: Option<PathBuf>,
}

impl MigrationStatus {
    /// The file was applied and has been edited since.
    pub fn drifted(&self) -> bool {
        matches!(&self.applied_checksum, Some(applied) if applied != &self.checksum)
    }
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
            source: f.source.clone(),
            checksum: f.checksum.clone(),
            applied_at: applied
                .iter()
                .find(|a| a.name == f.name)
                .map(|a| a.applied_at.clone()),
            applied_checksum: applied
                .iter()
                .find(|a| a.name == f.name)
                .map(|a| a.checksum.clone()),
            path: Some(f.path.clone()),
        })
        .collect();
    for a in applied {
        if !files.iter().any(|f| f.name == a.name) {
            out.push(MigrationStatus {
                name: a.name,
                // Applied, and no file on disk claims it any more.
                source: "(gone from the project)".to_owned(),
                checksum: a.checksum.clone(),
                applied_at: Some(a.applied_at),
                applied_checksum: Some(a.checksum),
                path: None,
            });
        }
    }
    Ok(out)
}

/// The pragma that takes a migration out of its transaction, for the
/// statements PostgreSQL refuses to run inside one — `CREATE INDEX
/// CONCURRENTLY` above all, which on a large table is the difference
/// between a background build and a write outage for its duration. It is a
/// line of its own, anywhere in the file:
///
/// ```sql
/// -- usai: no-transaction
/// create index concurrently if not exists invoices_due_at_idx on invoices (due_at);
/// ```
///
/// The ledger row cannot then be written atomically with the work, so a
/// process that dies between the two runs the file again: **the SQL must be
/// idempotent**. `IF NOT EXISTS` is how, and a concurrent index build that
/// fails leaves an invalid index that must be dropped by hand before the
/// retry (PostgreSQL's rule, not this runtime's).
pub fn runs_outside_a_transaction(sql: &str) -> bool {
    sql.lines().any(|line| {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("--") else {
            return false;
        };
        let rest = rest.trim();
        let Some(rest) = rest.strip_prefix("usai:") else {
            return false;
        };
        rest.trim().eq_ignore_ascii_case("no-transaction")
    })
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
        let unwrapped = runs_outside_a_transaction(&sql);
        tracing::info!(migration = %file.name, transactional = !unwrapped, "applying");
        let applied = if unwrapped {
            pg.apply_migration_unwrapped(&file.name, &sql, &file.checksum, cancel.clone())
                .await?
        } else {
            pg.apply_migration(&file.name, &sql, &file.checksum, cancel.clone())
                .await?
        };
        if applied {
            done.push(file.name.clone());
        } else {
            tracing::info!(migration = %file.name, "already applied by another migrator");
        }
    }
    Ok(done)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::definition::ModuleSpec;

    fn module(name: &str, glob: &str, source_dir: Option<&str>) -> ModuleSpec {
        ModuleSpec {
            name: name.to_owned(),
            migrations: vec![glob.to_owned()],
            seeders: Vec::new(),
            source_dir: source_dir.map(str::to_owned),
        }
    }

    /// A module's glob may be written relative to the module's own file. The
    /// glob as written is still tried first, so an application written
    /// against the old root-relative rule is unaffected.
    #[test]
    fn a_module_glob_is_tried_as_written_and_then_next_to_the_module() {
        let dir = std::env::temp_dir().join(format!("usai-globs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src/billing/migrations")).unwrap();
        std::fs::write(dir.join("src/billing/migrations/001_a.sql"), "select 1;").unwrap();

        // Written relative to the module file.
        let relative = migration_globs_for(
            &[module("billing", "./migrations/*.sql", Some("src/billing"))],
            &[],
        );
        let found = discover_migrations(&dir, &relative).unwrap();
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].name, "001_a.sql");

        // Written relative to the project root, as every application before
        // this one had to.
        let rooted = migration_globs_for(
            &[module(
                "billing",
                "./src/billing/migrations/*.sql",
                Some("src/billing"),
            )],
            &[],
        );
        let found = discover_migrations(&dir, &rooted).unwrap();
        assert_eq!(
            found.len(),
            1,
            "the root-relative form still works: {found:?}"
        );

        // The same file found both ways is one migration, not two.
        let both = migration_globs_for(
            &[
                module("billing", "./migrations/*.sql", Some("src/billing")),
                module(
                    "billing",
                    "./src/billing/migrations/*.sql",
                    Some("src/billing"),
                ),
            ],
            &[],
        );
        let found = discover_migrations(&dir, &both).unwrap();
        assert_eq!(
            found.len(),
            1,
            "a duplicate path is not a duplicate migration: {found:?}"
        );

        // A module with no stamped directory behaves exactly as before.
        let legacy = migration_globs_for(&[module("billing", "./migrations/*.sql", None)], &[]);
        assert_eq!(legacy.len(), 1, "no fallback without a source directory");
        assert!(discover_migrations(&dir, &legacy).unwrap().is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
