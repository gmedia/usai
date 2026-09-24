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
    root.join("node_modules/@sakaladev/usai")
        .exists()
        .then_some(root)
}

fn out_dir(tag: &str) -> PathBuf {
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    std::env::temp_dir().join(format!(
        "usai-project-{tag}-{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    ))
}

/// An application's identity is `sha256(manifest as this runtime serializes
/// it) + the code hash`, so a manifest written by the **previous** SDK has to
/// serialize back byte-identically here — otherwise the same artifact has a
/// different identity on each side of an upgrade, and a rolling deploy sees
/// two applications where there is one. That is the shape of the N−1 promise
/// in `SUPPORTED.md`.
///
/// It is broken by adding an `Option<T>` field without `skip_serializing_if`
/// — and, less obviously, by **removing** a field the previous SDK wrote,
/// which is how 0.0.10 changed the identity of every application declaring a
/// cron: the deduplicated `trigger.timeout_ms` stopped being serialized, so
/// the same artifact hashed differently on each side. The fixture that was
/// supposed to catch it held two HTTP routes and nothing else, which is why
/// this reads **every** manifest in the directory and why the one that
/// matters declares every trigger kind the SDK can produce.
#[test]
fn a_manifest_from_the_previous_sdk_round_trips_unchanged() {
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/manifests");
    let mut fixtures: Vec<PathBuf> = std::fs::read_dir(&dir)
        .expect("the manifest fixtures")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    fixtures.sort();
    assert!(
        fixtures.len() >= 2,
        "the fixtures directory lost its manifests"
    );

    for path in fixtures {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let raw = std::fs::read(&path).expect("the manifest fixture");
        let manifest: Manifest =
            serde_json::from_slice(&raw).unwrap_or_else(|e| panic!("{name} parses here: {e}"));
        let before: Value = serde_json::from_slice(&raw).expect("as a value");
        let after: Value =
            serde_json::from_slice(&serde_json::to_vec(&manifest).expect("it serializes"))
                .expect("as a value");
        let mut differences = Vec::new();
        diff_json("", &before, &after, &mut differences);
        assert!(
            differences.is_empty(),
            "{name} does not round-trip through this runtime, so every artifact the \
             previous SDK built changes identity on upgrade:\n  {}\n\
             A new Option field needs #[serde(default, skip_serializing_if = \"Option::is_none\")]; \
             a field the previous SDK wrote has to keep being written even when nothing reads it.",
            differences.join("\n  ")
        );
    }
}

/// Every place the two documents differ, as a JSON pointer and what happened
/// there. The failure is always "this runtime added, dropped or rewrote a
/// field", and naming the pointer is the difference between a one-line fix
/// and an afternoon.
fn diff_json(at: &str, before: &Value, after: &Value, out: &mut Vec<String>) {
    match (before, after) {
        (Value::Object(a), Value::Object(b)) => {
            for (key, value) in a {
                let at = format!("{at}/{key}");
                match b.get(key) {
                    Some(other) => diff_json(&at, value, other, out),
                    None => out.push(format!("{at}: dropped (was {value})")),
                }
            }
            for key in b.keys().filter(|k| !a.contains_key(*k)) {
                out.push(format!("{at}/{key}: added (now {})", b[key]));
            }
        }
        (Value::Array(a), Value::Array(b)) if a.len() == b.len() => {
            for (i, (x, y)) in a.iter().zip(b).enumerate() {
                diff_json(&format!("{at}/{i}"), x, y, out);
            }
        }
        _ if before != after => out.push(format!("{at}: {before} became {after}")),
        _ => {}
    }
}

/// An artifact directory is a deploy mount and a rollback target, so a
/// failed build must not touch it. It used to: the bundler wrote `app.js`
/// first and a fault in `describe()` left that new bundle beside the
/// previous `manifest.json` — which the runtime refuses to serve, correctly
/// and permanently, until the next *successful* build.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_failed_build_leaves_the_previous_artifact_runnable() {
    let Some(root) = root() else { return };
    let engine = usai_runtime::engine::from_env(64).unwrap();
    let config = load_config(engine.as_ref(), &root).await.unwrap();
    let out = out_dir("atomic");
    build(
        engine.as_ref(),
        &BuildOptions {
            out_dir: out.clone(),
            ..BuildOptions::from_config(&config)
        },
    )
    .await
    .expect("the first build");
    let good = usai_runtime::build::load_artifact(&out)
        .await
        .expect("the artifact this build wrote");

    // An entry that bundles and then faults before the manifest exists.
    let bad_entry = out
        .join("..")
        .join(format!("bad-entry-{}.ts", std::process::id()));
    tokio::fs::write(&bad_entry, "throw new Error(\"boom\");\n")
        .await
        .unwrap();
    let failure = build(
        engine.as_ref(),
        &BuildOptions {
            out_dir: out.clone(),
            entry: bad_entry.canonicalize().unwrap(),
            ..BuildOptions::from_config(&config)
        },
    )
    .await;
    let error = match failure {
        Ok(_) => panic!("the bad entry built"),
        Err(e) => e.to_string(),
    };
    // The point of the test is a failure *after* the bundler wrote a file.
    // If the bundler itself had refused, nothing would have been written and
    // the assertion below would hold for the wrong reason.
    assert!(
        !error.contains("bundler") && !error.contains("esbuild"),
        "the bundler refused, so this proves nothing: {error}"
    );

    // The directory still holds a coherent artifact: same identity, and it
    // loads — which is the integrity check the runtime runs before serving.
    let after = usai_runtime::build::load_artifact(&out)
        .await
        .expect("the previous artifact survived the failed build");
    assert_eq!(after.identity(), good.identity());
    let _ = tokio::fs::remove_file(&bad_entry).await;
    let _ = tokio::fs::remove_dir_all(&out).await;
}

/// A mistake in `defineApp` is a **declaration** mistake, and it has to read
/// like one. Declarations are evaluated inside the guest, so every one of
/// them arrives as `guest fault: Error: …` unless the build unwraps it —
/// which sends the reader to the engine, the bundle, or the runtime, for a
/// line they wrote in their own application.
///
/// The unwrapping was applied to the validator warm-up and not to the
/// manifest extraction that runs a phase later, so a resource declared twice
/// read as itself while two auth schemes sharing a name — thrown by
/// `describe()` — still carried the prefix the release said it had removed.
/// The two mistakes are the same kind of mistake.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_declaration_mistake_never_reads_as_a_guest_fault() {
    let Some(root) = root() else { return };
    let engine = usai_runtime::engine::from_env(64).unwrap();
    let config = load_config(engine.as_ref(), &root).await.unwrap();

    // Two modules, each calling its scheme `session` and meaning something
    // different by it: the default outcome when two teams share a codebase,
    // not an exotic one. It is thrown by `describe()`, after the warm-up.
    let entry = root.join(format!("collide-{}.ts", std::process::id()));
    tokio::fs::write(
        &entry,
        r#"import { auth, defineApp, defineModule, http } from "@sakaladev/usai";
const a = auth.bearer({ name: "session", resolve: async () => ({ id: "a" }) });
const b = auth.cookie({ name: "session", cookie: "sid", resolve: async () => ({ id: "b" }) });
const one = defineModule({ name: "one", workloads: [http.get("/a", { auth: a }, async () => ({}))] });
const two = defineModule({ name: "two", workloads: [http.get("/b", { auth: b }, async () => ({}))] });
export default defineApp({ name: "collide", modules: [one, two] });
"#,
    )
    .await
    .unwrap();
    let out = out_dir("collide");
    let failure = build(
        engine.as_ref(),
        &BuildOptions {
            out_dir: out.clone(),
            entry: entry.canonicalize().unwrap(),
            ..BuildOptions::from_config(&config)
        },
    )
    .await;
    // Clean up *before* asserting: the entry lives in the fixture project
    // because it has to resolve `@sakaladev/usai`, so a failing assertion
    // that panicked first left a stray file in the repository.
    let error = failure.err().map(|e| e.to_string());
    let _ = tokio::fs::remove_file(&entry).await;
    let _ = tokio::fs::remove_dir_all(&out).await;
    let error = error.expect("two auth schemes with one name built");

    assert!(
        error.contains(r#"auth scheme "session" is declared twice"#),
        "the reason is what the declaration says: {error}"
    );
    assert!(
        error.contains(r#"module "one""#) && error.contains(r#"module "two""#),
        "a conflict names both declarers: {error}"
    );
    assert!(
        !error.contains("guest fault"),
        "a declaration mistake still reads as a guest fault: {error}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn config_and_module_metadata_compose_deterministically() {
    let Some(root) = root() else { return };
    let engine = usai_runtime::engine::from_env(64).unwrap();
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
    let engine = usai_runtime::engine::from_env(64).unwrap();
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
    let engine = usai_runtime::engine::from_env(64).unwrap();
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

    // Migrate: three files from three directories, in name order. Four
    // migrators at once (two replicas' migrate jobs and two operators) apply
    // each file exactly once between them: the advisory lock serializes
    // them and the ledger row goes in before the SQL, so a loser never runs
    // a migration twice.
    let globs = db::migration_globs(&out.definition, &config.migrations.value);
    let files = db::discover_migrations(&root, &globs).unwrap();
    let manager = db::database(&rev, None).unwrap();
    let mut racers = Vec::new();
    for _ in 0..4 {
        let m = Arc::clone(&manager);
        let files = files.clone();
        racers.push(tokio::spawn(async move {
            db::migrate(m.as_ref(), &files, CancellationToken::new()).await
        }));
    }
    let mut applied: Vec<String> = Vec::new();
    for r in racers {
        applied.extend(r.await.unwrap().unwrap());
    }
    applied.sort();
    assert_eq!(
        applied,
        vec!["001_users.sql", "002_invoices.sql", "003_audit.sql"],
        "each migration applied by exactly one migrator"
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

/// Several `usai` processes on one project build into one `.usai/build`, and
/// the staging directory is a single path inside it — so the first thing
/// each build does (`remove_dir_all(.staging)`) deleted the files the other
/// was writing, and a reader mid-rename loaded half a bundle. This is not an
/// exotic case: `usai test` runs test files **in parallel** and each
/// `testApp({ migrate: true })` shells out to `usai db migrate`, so a
/// project's second test file was enough. Measured before the lock: four
/// concurrent `usai inspect` on a cold project, one succeeded and three
/// failed with a bare `io: No such file or directory`.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_builds_of_one_project_cooperate() {
    let Some(root) = root() else { return };
    let out = out_dir("concurrent");
    let options = BuildOptions {
        out_dir: out.clone(),
        ..BuildOptions::for_project(&root)
    };
    let mut set = tokio::task::JoinSet::new();
    for _ in 0..4 {
        let options = options.clone();
        set.spawn(async move {
            let engine = usai_runtime::engine::from_env(8).unwrap();
            usai_runtime::build::load_or_build(engine.as_ref(), &options)
                .await
                .map(|d| d.name().to_owned())
        });
    }
    let mut names = Vec::new();
    while let Some(joined) = set.join_next().await {
        names.push(joined.expect("task").expect("every build succeeds"));
    }
    assert_eq!(names.len(), 4);
    assert!(
        names.windows(2).all(|w| w[0] == w[1]),
        "the four builds did not agree on the application: {names:?}"
    );
    let _ = std::fs::remove_dir_all(&out);
}

/// A module **shipped as a package** — the way a shared layer is actually
/// shared — must own its migrations the way a module inside `src/` does.
/// The bundler skipped `node_modules` when it stamped each module's source
/// directory, so an installed module's `./migrations/*.sql` resolved from
/// nowhere: `matches no file`, no `migrations/` in the artifact, and
/// `usai db migrate` reporting success. A green deploy whose first request
/// says `column does not exist`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_module_installed_as_a_package_still_owns_its_migrations() {
    let Some(fixture) = root() else { return };
    // The SDK the fixture resolves, reached through its own node_modules.
    let sdk = fixture.join("node_modules/@sakaladev/usai");
    let Ok(sdk) = std::fs::canonicalize(&sdk) else {
        return;
    };
    let project = std::env::temp_dir().join(format!("usai-vendored-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&project);
    let module = project.join("node_modules/@acme/notes/src/migrations");
    std::fs::create_dir_all(&module).unwrap();
    std::fs::create_dir_all(project.join("src")).unwrap();
    std::fs::create_dir_all(project.join("node_modules/@sakaladev")).unwrap();
    std::os::unix::fs::symlink(&sdk, project.join("node_modules/@sakaladev/usai")).unwrap();
    std::fs::write(
        project.join("package.json"),
        r#"{"name":"vendored","private":true,"type":"module"}"#,
    )
    .unwrap();
    std::fs::write(
        project.join("node_modules/@acme/notes/package.json"),
        r#"{"name":"@acme/notes","version":"1.0.0","type":"module","exports":{".":"./src/module.ts"}}"#,
    )
    .unwrap();
    std::fs::write(
        project.join("node_modules/@acme/notes/src/module.ts"),
        "import { defineModule, http } from \"@sakaladev/usai\";\n\
         export const notes = defineModule({\n\
         \x20 name: \"notes\",\n\
         \x20 migrations: \"./migrations/*.sql\",\n\
         \x20 workloads: [http.get(\"/notes/ping\", {}, async () => ({ ok: true }))],\n\
         });\n",
    )
    .unwrap();
    std::fs::write(
        module.join("004_vendor_notes.sql"),
        "create table if not exists vendor_notes (id bigserial primary key);",
    )
    .unwrap();
    std::fs::write(
        project.join("src/app.ts"),
        "import { defineApp } from \"@sakaladev/usai\";\n\
         import { notes } from \"@acme/notes\";\n\
         export default defineApp({ name: \"vendored\", modules: [notes] });\n",
    )
    .unwrap();

    let out = out_dir("vendored");
    let engine = usai_runtime::engine::from_env(8).unwrap();
    let built = build(
        engine.as_ref(),
        &BuildOptions {
            out_dir: out.clone(),
            ..BuildOptions::for_project(&project)
        },
    )
    .await
    .expect("the project builds");
    let module_spec = built
        .definition
        .manifest()
        .modules
        .iter()
        .find(|m| m.name == "notes")
        .expect("the module is in the manifest");
    let source_dir = module_spec
        .source_dir
        .as_deref()
        .expect("a module reached through node_modules is stamped like any other");
    assert!(
        source_dir.contains("node_modules/@acme/notes/src"),
        "{source_dir}"
    );
    assert!(
        out.join("migrations/004_vendor_notes.sql").exists(),
        "the package's SQL must travel into the artifact"
    );
    let _ = std::fs::remove_dir_all(&project);
    let _ = std::fs::remove_dir_all(&out);
}
