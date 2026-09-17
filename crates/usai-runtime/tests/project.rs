//! D7 acceptance: project model — modules, migrations, seeders, typed env,
//! config. Centralized and colocated layouts both work; organization does
//! not change semantics.

mod support;

use std::path::PathBuf;
use std::sync::Arc;

use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;
use usai_runtime::build::{BuildOptions, build, build_seeder, load_config};
use usai_runtime::db;
use usai_runtime::*;

fn root() -> Option<PathBuf> {
    if std::process::Command::new("node")
        .arg("--version")
        .output()
        .map(|o| !o.status.success())
        .unwrap_or(true)
    {
        return None;
    }
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/project-app");
    root.join("node_modules/usai").exists().then_some(root)
}

fn out_dir(tag: &str) -> PathBuf {
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    std::env::temp_dir().join(format!(
        "usai-project-{tag}-{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    ))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn config_and_module_metadata_compose_deterministically() {
    let Some(root) = root() else { return };
    let engine = QuickJsEngine::new(QuickJsConfig::default());
    let config = load_config(engine.as_ref(), &root).await.unwrap();
    assert_eq!(config.app.source, "usai.config.ts");
    assert_eq!(
        config.migrations.value,
        vec!["./src/**/migrations/*.sql", "./migrations/*.sql"]
    );
    assert_eq!(config.out_dir.source, "default");

    let out = build(
        engine.as_ref(),
        &BuildOptions {
            out_dir: out_dir("meta"),
            ..BuildOptions::from_config(&config)
        },
    )
    .await
    .unwrap();
    let m = out.definition.manifest();
    assert_eq!(
        m.modules
            .iter()
            .map(|x| x.name.as_str())
            .collect::<Vec<_>>(),
        vec!["users", "billing"]
    );
    assert_eq!(
        m.modules[0].migrations,
        vec!["./src/users/migrations/*.sql"]
    );
    assert_eq!(m.modules[0].seeders, vec!["./src/users/seeders/*.ts"]);
    assert_eq!(
        m.resources.len(),
        1,
        "one resource declared in two modules is one resource"
    );
    assert_eq!(m.resources[0].module.as_deref(), Some("users"));
    assert_eq!(
        out.definition
            .workload("http:GET /users")
            .unwrap()
            .1
            .module
            .as_deref(),
        Some("users")
    );

    let globs = db::migration_globs(&out.definition, &config.migrations.value);
    let files = db::discover_migrations(&root, &globs).unwrap();
    assert_eq!(
        files.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(),
        vec!["001_users.sql", "002_invoices.sql", "003_audit.sql"]
    );
    assert!(
        files[0]
            .path
            .ends_with("src/users/migrations/001_users.sql")
    );
    assert!(files[2].path.ends_with("migrations/003_audit.sql"));
    let seeders = db::discover_seeders(
        &root,
        &db::seeder_globs(&out.definition, &config.seeders.value),
    )
    .unwrap();
    assert_eq!(
        seeders.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(),
        vec!["demo", "dev"]
    );
    // Building twice yields the same identity: same source, same definition.
    let again = build(
        engine.as_ref(),
        &BuildOptions {
            out_dir: out_dir("meta2"),
            ..BuildOptions::from_config(&config)
        },
    )
    .await
    .unwrap();
    assert_eq!(out.definition.identity(), again.definition.identity());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn typed_env_fails_activation_on_shape_not_first_request() {
    let Some(root) = root() else { return };
    let engine = QuickJsEngine::new(QuickJsConfig::default());
    let config = load_config(engine.as_ref(), &root).await.unwrap();
    let out = build(
        engine.as_ref(),
        &BuildOptions {
            out_dir: out_dir("env"),
            ..BuildOptions::from_config(&config)
        },
    )
    .await
    .unwrap();
    let def = out.definition;
    let attempt = |vars: Vec<(&'static str, &'static str)>| {
        let engine = Arc::clone(&engine);
        let def = Arc::clone(&def);
        async move {
            let runtime = Runtime::with_env(
                engine,
                RuntimeConfig {
                    cron_scheduler: false,
                    ..RuntimeConfig::default()
                },
                move |n| {
                    vars.iter()
                        .find(|(k, _)| *k == n)
                        .map(|(_, v)| (*v).to_owned())
                },
            );
            let rev = runtime.install(def).await.unwrap();
            runtime
                .activate(rev.id)
                .await
                .map(|_| ())
                .map_err(|e| e.to_string())
        }
    };
    let err = attempt(vec![("APP_ENV", "development"), ("WORKERS", "4")])
        .await
        .unwrap_err();
    assert!(err.contains("DATABASE_URL"), "{err}");
    let err = attempt(vec![
        ("DATABASE_URL", "postgres://x"),
        ("APP_ENV", "staging"),
        ("WORKERS", "4"),
    ])
    .await
    .unwrap_err();
    assert!(
        err.contains("APP_ENV") && err.contains("development"),
        "{err}"
    );
    let err = attempt(vec![
        ("DATABASE_URL", "postgres://x"),
        ("APP_ENV", "development"),
        ("WORKERS", "four"),
    ])
    .await
    .unwrap_err();
    assert!(err.contains("WORKERS"), "{err}");
    let err = attempt(vec![
        ("DATABASE_URL", "not a url"),
        ("APP_ENV", "development"),
        ("WORKERS", "4"),
    ])
    .await
    .unwrap_err();
    assert!(err.contains("DATABASE_URL"), "{err}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn migrations_seeders_and_typed_env_end_to_end() {
    let Some(root) = root() else { return };
    let Some(server) = support::database_url() else {
        return;
    };
    let url = support::fresh_database(&server).await;
    let engine = QuickJsEngine::new(QuickJsConfig::default());
    let config = load_config(engine.as_ref(), &root).await.unwrap();
    let options = BuildOptions {
        out_dir: out_dir("e2e"),
        ..BuildOptions::from_config(&config)
    };
    let out = build(engine.as_ref(), &options).await.unwrap();
    let env_url = url.clone();
    let env = move |n: &str| match n {
        "DATABASE_URL" => Some(env_url.clone()),
        "APP_ENV" => Some("development".to_owned()),
        "WORKERS" => Some("4".to_owned()),
        "DEBUG" => Some("true".to_owned()),
        _ => None,
    };
    let runtime = Runtime::with_env(
        Arc::clone(&engine) as Arc<dyn usai_runtime::engine::Engine>,
        RuntimeConfig {
            cron_scheduler: false,
            ..RuntimeConfig::default()
        },
        env.clone(),
    );
    let rev = runtime.install(Arc::clone(&out.definition)).await.unwrap();
    runtime.activate(rev.id).await.unwrap();

    // Typed env inside the world.
    let r = runtime.invoke("http:GET /config", json!({ "kind": "http", "env": {}, "request": { "method": "GET", "path": "/config", "url": "/config", "params": {}, "query": {}, "headers": {}, "body": null } })).await.unwrap();
    let v = r.outcome.unwrap().unwrap();
    assert_eq!(v["json"]["env"]["WORKERS"], 4);
    assert_eq!(v["json"]["env"]["DEBUG"], true);
    assert_eq!(v["json"]["env"]["APP_ENV"], "development");

    // Migrate: three files from three directories, in name order.
    let globs = db::migration_globs(&out.definition, &config.migrations.value);
    let files = db::discover_migrations(&root, &globs).unwrap();
    let manager = db::database(&rev, None).unwrap();
    let applied = db::migrate(manager.as_ref(), &files, CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(
        applied,
        vec!["001_users.sql", "002_invoices.sql", "003_audit.sql"]
    );
    let again = db::migrate(manager.as_ref(), &files, CancellationToken::new())
        .await
        .unwrap();
    assert!(again.is_empty(), "migrations are applied once");
    let status = db::status(manager.as_ref(), &files).await.unwrap();
    assert!(status.iter().all(|s| s.applied_at.is_some()));

    // A changed applied migration is refused.
    let mut tampered = files.clone();
    tampered[0].checksum = "deadbeefdeadbeef".into();
    let err = db::migrate(manager.as_ref(), &tampered, CancellationToken::new())
        .await
        .unwrap_err();
    assert!(matches!(err, db::DbError::ChecksumMismatch { .. }), "{err}");

    // Seed: colocated `dev` then centralized `demo`, each a finite world.
    let seeders = db::discover_seeders(
        &root,
        &db::seeder_globs(&out.definition, &config.seeders.value),
    )
    .unwrap();
    for name in ["dev", "demo"] {
        let seeder = seeders.iter().find(|s| s.name == name).unwrap();
        let built = build_seeder(engine.as_ref(), &options, &seeder.path, name)
            .await
            .unwrap();
        assert!(
            built
                .definition
                .workload(&format!("command:seed:{name}"))
                .is_some()
        );
        let seed_runtime = Runtime::with_env(
            Arc::clone(&engine) as Arc<dyn usai_runtime::engine::Engine>,
            RuntimeConfig {
                cron_scheduler: false,
                ..RuntimeConfig::default()
            },
            env.clone(),
        );
        let srev = seed_runtime.install(built.definition).await.unwrap();
        seed_runtime.activate(srev.id).await.unwrap();
        let r = seed_runtime
            .run_command(&format!("seed:{name}"), vec![])
            .await
            .unwrap();
        assert!(
            matches!(r.outcome, Some(Ok(_))),
            "seeder {name}: {:?}",
            r.outcome
        );
        if name == "dev" {
            assert_eq!(r.logs[0].message, "seeded users");
        }
        seed_runtime.shutdown().await;
    }
    let r = runtime.invoke("http:GET /users", json!({ "kind": "http", "env": {}, "request": { "method": "GET", "path": "/users", "url": "/users", "params": {}, "query": {}, "headers": {}, "body": null } })).await.unwrap();
    let users = r.outcome.unwrap().unwrap()["json"].clone();
    assert_eq!(
        users,
        json!([{ "id": 1, "name": "Ayu" }, { "id": 2, "name": "Budi" }])
    );
    let invoices = manager
        .call(usai_runtime::resource::ResourceCall { method: "one".into(), args: json!({ "sql": "select count(*)::int as n, sum(amount)::text as total from invoices", "params": [] }) }, CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(invoices, json!({ "n": 2, "total": "21.00" }));
    runtime.shutdown().await;
    let _ = Value::Null;
}
