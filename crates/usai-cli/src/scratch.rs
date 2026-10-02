//! Throwaway databases for `testApp({ database: "fresh" })`.
//!
//! `create` makes one empty database per connection-URL variable the
//! application's `postgres` resources read, on the same server, and prints
//! `{ "<VAR>": "<url of the new database>" }`. `drop` removes them. Both only
//! ever touch databases named `usai_scratch_…`, so a wrong URL can at worst
//! fail — it can never empty a database somebody else made.

use std::collections::BTreeMap;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context as _, Result};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;
use usai_runtime::build::{BuildOptions, load_config};
use usai_runtime::definition::ResourceSpec;
use usai_runtime::resource::{ResourceCall, ResourceManager, ResourceRegistry};

const PREFIX: &str = "usai_scratch_";

/// A run killed before it could drop its databases (a CI cancel, `kill -9`)
/// leaves them behind; the next `create` removes the ones older than this.
const LEFTOVER_AGE_SECONDS: u64 = 24 * 60 * 60;

pub async fn create(root: &Path) -> Result<()> {
    let mut out = BTreeMap::new();
    for var in url_variables(root).await? {
        let base = std::env::var(&var).with_context(|| {
            format!("{var} is not set: a fresh database is made on the server it points at")
        })?;
        let now = SystemTime::now().duration_since(UNIX_EPOCH)?;
        let name = format!(
            "{PREFIX}{}_{}_{}",
            now.as_secs(),
            std::process::id(),
            now.subsec_nanos() % 1_000_000
        );
        with_server(&base, async |admin| {
            drop_leftovers(admin, now.as_secs()).await;
            call(admin, "execute", &format!("CREATE DATABASE {name}")).await
        })
        .await
        .with_context(|| format!("creating a fresh database next to {var}"))?;
        out.insert(var, with_database(&base, &name)?);
    }
    println!("{}", serde_json::to_string(&out)?);
    Ok(())
}

/// Drops the scratch databases these URLs name. The server is reached through
/// the URL itself, with the `postgres` maintenance database in its place.
pub async fn drop(urls: &[String]) -> Result<()> {
    for url in urls {
        let name = database_of(url)?;
        anyhow::ensure!(
            name.starts_with(PREFIX),
            "{name} is not a scratch database; only {PREFIX}* are ever dropped"
        );
        let admin = with_database(url, "postgres")?;
        with_server(&admin, async |admin| {
            call(
                admin,
                "execute",
                &format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)"),
            )
            .await
        })
        .await
        .with_context(|| format!("dropping {name}"))?;
    }
    Ok(())
}

/// The distinct `urlEnv`s of the application's postgres resources.
async fn url_variables(root: &Path) -> Result<Vec<String>> {
    let engine = crate::commands::engine();
    let config = load_config(engine.as_ref(), root).await?;
    let built =
        usai_runtime::build::build(engine.as_ref(), &BuildOptions::from_config(&config)).await?;
    let mut vars: Vec<String> = built
        .definition
        .resources()
        .iter()
        .filter(|r| r.kind == "postgres")
        .map(|r| {
            r.config["urlEnv"]
                .as_str()
                .unwrap_or("DATABASE_URL")
                .to_owned()
        })
        .collect();
    vars.sort();
    vars.dedup();
    anyhow::ensure!(
        !vars.is_empty(),
        "this application declares no postgres resource, so there is no database to make fresh"
    );
    Ok(vars)
}

/// Runs `f` on a connection to the server `url` points at. When that
/// database does not exist yet (a `myapp_test` nobody created), the
/// `postgres` maintenance database on the same server is used instead.
async fn with_server<T>(
    url: &str,
    f: impl AsyncFnOnce(&dyn ResourceManager) -> Result<T>,
) -> Result<T> {
    const VAR: &str = "USAI_SCRATCH_ADMIN_URL";
    let spec = ResourceSpec {
        name: "scratch".into(),
        kind: "postgres".into(),
        module: None,
        config: json!({ "urlEnv": VAR }),
        env: vec![VAR.into()],
    };
    let registry = ResourceRegistry::new();
    let open = |url: String| {
        let registry = &registry;
        let spec = &spec;
        async move {
            registry
                .open(spec, &|name| (name == VAR).then(|| url.clone()))
                .await
        }
    };
    let manager = match open(url.to_owned()).await {
        Ok(manager) => manager,
        Err(first) if first.to_string().contains("does not exist") => {
            open(with_database(url, "postgres")?)
                .await
                .map_err(|_| first)?
        }
        Err(e) => return Err(e.into()),
    };
    let result = f(manager.as_ref()).await;
    registry.shutdown().await;
    result
}

async fn call(manager: &dyn ResourceManager, method: &str, sql: &str) -> Result<Value> {
    Ok(manager
        .call(
            ResourceCall {
                method: method.into(),
                args: json!({ "sql": sql, "params": [] }),
            },
            CancellationToken::new(),
        )
        .await?)
}

/// Best effort: a leftover still in use, or one this role cannot drop, stays.
async fn drop_leftovers(admin: &dyn ResourceManager, now: u64) {
    let Ok(rows) = call(
        admin,
        "query",
        "SELECT datname FROM pg_database WHERE datname LIKE 'usai\\_scratch\\_%'",
    )
    .await
    else {
        return;
    };
    for row in rows.as_array().into_iter().flatten() {
        let Some(name) = row["datname"].as_str() else {
            continue;
        };
        let created = name[PREFIX.len()..]
            .split('_')
            .next()
            .and_then(|s| s.parse::<u64>().ok());
        if created.is_some_and(|at| now.saturating_sub(at) > LEFTOVER_AGE_SECONDS) {
            let _ = call(admin, "execute", &format!("DROP DATABASE IF EXISTS {name}")).await;
        }
    }
}

/// The URL with its database replaced. URL form only: a key/value
/// connection string has no single place to put a database name back.
fn with_database(url: &str, database: &str) -> Result<String> {
    let (head, _, query) = split(url)?;
    Ok(format!("{head}/{database}{query}"))
}

fn database_of(url: &str) -> Result<String> {
    let (_, path, _) = split(url)?;
    anyhow::ensure!(!path.is_empty(), "{url} names no database");
    Ok(path.to_owned())
}

/// `postgres://user:pw@host:5432` · `mydb` · `?sslmode=require`.
fn split(url: &str) -> Result<(&str, &str, &str)> {
    let scheme_end = ["postgres://", "postgresql://"]
        .iter()
        .find(|s| url.starts_with(**s))
        .map(|s| s.len())
        .context("a fresh database needs a postgres:// URL (not a key=value connection string)")?;
    let (base, query) = match url.find('?') {
        Some(i) => (&url[..i], &url[i..]),
        None => (url, ""),
    };
    match base[scheme_end..].find('/') {
        Some(i) => Ok((&base[..scheme_end + i], &base[scheme_end + i + 1..], query)),
        None => Ok((base, "", query)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn database_swaps_keep_credentials_and_options() {
        let url = "postgres://u:p@db:5432/app_test?sslmode=require";
        assert_eq!(database_of(url).unwrap(), "app_test");
        assert_eq!(
            with_database(url, "usai_scratch_1").unwrap(),
            "postgres://u:p@db:5432/usai_scratch_1?sslmode=require"
        );
        assert_eq!(
            with_database("postgresql://db", "postgres").unwrap(),
            "postgresql://db/postgres"
        );
        assert!(with_database("host=db dbname=x", "y").is_err());
    }
}
