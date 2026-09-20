//! Human-readable rendering of the ApplicationDefinition (`GOAL.md` §38–§39).

use std::fmt::Write as _;

use usai_runtime::definition::{ApplicationDefinition, LifetimeFamily, Trigger};
use usai_runtime::runtime::RuntimeStatus;

fn lifetime_label(family: LifetimeFamily, trigger: &Trigger) -> &'static str {
    match (family, trigger) {
        (_, Trigger::Http { .. }) => "request",
        (_, Trigger::Task) => "task",
        (_, Trigger::Cron { .. }) => "invocation",
        (_, Trigger::Command) => "invocation",
        (_, Trigger::Queue { .. }) => "message",
        (_, Trigger::Socket { .. }) => "connection",
        (_, Trigger::Stream { .. }) => "stream",
        (_, Trigger::Service { .. }) => "service",
    }
}

/// The compact banner `usai dev` prints.
pub fn banner(
    definition: &ApplicationDefinition,
    revision: &str,
    base_url: Option<&str>,
    status: Option<&RuntimeStatus>,
    docs: bool,
    status_surface: bool,
) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "Usai\n");
    let _ = writeln!(out, "Application  {}", definition.name());
    let _ = writeln!(out, "Revision     {revision}");
    let m = definition.manifest();

    let http: Vec<_> = m
        .workloads
        .iter()
        .filter(|w| matches!(w.trigger, Trigger::Http { .. }))
        .collect();
    if !http.is_empty() {
        let _ = writeln!(out, "\nHTTP");
        for w in http {
            if let Trigger::Http {
                method, path, raw, ..
            } = &w.trigger
            {
                let _ = writeln!(
                    out,
                    "  {method:<6} {path}{}",
                    if *raw { "   (raw)" } else { "" }
                );
            }
        }
    }
    for (title, kind) in [("Streams", "stream"), ("WebSockets", "socket")] {
        let items: Vec<_> = m
            .workloads
            .iter()
            .filter(|w| w.trigger.kind_name() == kind)
            .collect();
        if !items.is_empty() {
            let _ = writeln!(out, "\n{title}");
            for w in items {
                match &w.trigger {
                    Trigger::Stream { method, path, .. } => {
                        let _ = writeln!(out, "  {method:<6} {path}");
                    }
                    Trigger::Socket { path } => {
                        let _ = writeln!(out, "  {:<6} {path}", "GET");
                    }
                    _ => {}
                }
            }
        }
    }
    for (title, kind) in [
        ("Tasks", "task"),
        ("Commands", "command"),
        ("Services", "service"),
        ("Queues", "queue"),
    ] {
        let items: Vec<_> = m
            .workloads
            .iter()
            .filter(|w| w.trigger.kind_name() == kind)
            .collect();
        if !items.is_empty() {
            let note = match (kind, status.map(|s| &s.scheduler)) {
                ("service", Some(s)) if !s.services => {
                    "   (not running on this instance: --no-services)"
                }
                ("queue", Some(s)) if !s.queue => "   (not consuming on this instance: --no-queue)",
                _ => "",
            };
            let _ = writeln!(out, "\n{title}{note}");
            for w in items {
                let _ = writeln!(out, "  {}", w.name);
            }
        }
    }
    let cron: Vec<_> = m
        .workloads
        .iter()
        .filter(|w| matches!(w.trigger, Trigger::Cron { .. }))
        .collect();
    if !cron.is_empty() {
        // Whether this instance ticks them is a fact about the instance,
        // not the application (several replicas: one scheduler).
        let scheduled = status.map(|s| s.scheduler.cron);
        let _ = writeln!(
            out,
            "\nCron{}",
            match scheduled {
                Some(false) => "   (not scheduled on this instance: --no-cron)",
                _ => "",
            }
        );
        for w in cron {
            if let Trigger::Cron {
                schedule,
                exclusive,
                ..
            } = &w.trigger
            {
                let _ = writeln!(
                    out,
                    "  {:<16} {schedule}{}",
                    w.name,
                    if *exclusive {
                        "   (exclusive: one instance per tick, claimed in the database)"
                    } else {
                        ""
                    }
                );
            }
        }
    }
    if !m.resources.is_empty() {
        let _ = writeln!(out, "\nResources");
        for r in &m.resources {
            let ready = status
                .map(|s| {
                    if s.resources
                        .iter()
                        .any(|st| st.identity.name == r.name && st.ready)
                    {
                        "ready"
                    } else {
                        "pending"
                    }
                })
                .unwrap_or("");
            let _ = writeln!(out, "  {:<16} {ready}", format!("{}/{}", r.kind, r.name));
        }
    }
    if let Some(url) = base_url {
        let _ = writeln!(out, "\nApp       {url}");
        if docs || status_surface {
            let _ = writeln!(out, "API Docs  {url}/_usai/docs");
            let _ = writeln!(out, "OpenAPI   {url}/_usai/openapi.json");
        }
        if status_surface {
            let _ = writeln!(out, "Status    {url}/_usai/status");
            let _ = writeln!(out, "Metrics   {url}/_usai/metrics");
        }
    }
    out
}

/// The full `usai inspect` view.
pub fn inspect(definition: &ApplicationDefinition) -> String {
    let mut out = String::new();
    let m = definition.manifest();
    let _ = writeln!(out, "Application: {}", definition.name());
    let _ = writeln!(out, "Identity:    {}", definition.identity());
    if !m.modules.is_empty() {
        let _ = writeln!(out, "\nModules");
        for module in &m.modules {
            let _ = writeln!(out, "  {}", module.name);
            for g in &module.migrations {
                let _ = writeln!(out, "    migrations: {g}");
            }
            for g in &module.seeders {
                let _ = writeln!(out, "    seeders:    {g}");
            }
        }
    }
    // Grouped by kind in a fixed order, whatever the declaration order.
    const KIND_ORDER: [&str; 8] = [
        "http", "stream", "socket", "task", "cron", "queue", "service", "command",
    ];
    let mut ordered: Vec<&_> = m.workloads.iter().collect();
    ordered.sort_by_key(|w| {
        KIND_ORDER
            .iter()
            .position(|k| *k == w.trigger.kind_name())
            .unwrap_or(KIND_ORDER.len())
    });
    let mut last_kind = "";
    for w in ordered {
        let kind = w.trigger.kind_name();
        if kind != last_kind {
            let title = match kind {
                "http" => "HTTP",
                "task" => "Tasks",
                "cron" => "Cron",
                "command" => "Commands",
                "service" => "Services",
                "queue" => "Queues",
                "socket" => "Sockets",
                "stream" => "Streams",
                _ => kind,
            };
            let _ = writeln!(out, "\n{title}");
            last_kind = kind;
        }
        match &w.trigger {
            Trigger::Http {
                method,
                path,
                raw,
                responses,
                ..
            } => {
                let _ = writeln!(
                    out,
                    "  {method} {path}{}",
                    if *raw {
                        "  (raw: exact bytes in and out; no input contract)"
                    } else {
                        ""
                    }
                );
                if *raw && !responses.is_empty() {
                    let listed: Vec<String> = responses
                        .iter()
                        .map(|(status, text)| format!("{status} {text}"))
                        .collect();
                    let _ = writeln!(out, "    responses (declared): {}", listed.join(", "));
                }
            }
            Trigger::Cron {
                schedule,
                overlap,
                exclusive,
                ..
            } => {
                let _ = writeln!(
                    out,
                    "  {}\n    schedule: {schedule}\n    overlap: {}{}",
                    w.name,
                    format!("{overlap:?}").to_lowercase(),
                    if *exclusive {
                        "\n    exclusive: one instance per tick (claimed in usai_cron_ticks)"
                    } else {
                        ""
                    }
                );
            }
            _ => {
                let _ = writeln!(out, "  {}", w.name);
            }
        }
        let _ = writeln!(
            out,
            "    lifetime: {}",
            lifetime_label(w.lifetime(), &w.trigger)
        );
        if let Some(module) = &w.module {
            let _ = writeln!(out, "    module: {module}");
        }
        let c = &w.contracts;
        for (slot, present) in [
            ("params", c.params.is_some()),
            ("query", c.query.is_some()),
            ("headers", c.headers.is_some()),
            ("body", c.body.is_some()),
            ("input", c.input.is_some()),
            ("message", c.message.is_some()),
        ] {
            if present {
                let once = c.boundary_final.iter().any(|f| f == slot);
                let _ = writeln!(
                    out,
                    "    {slot}: validated before world creation{}",
                    if once {
                        " (once; the world only strips undeclared keys)"
                    } else {
                        ""
                    }
                );
            }
        }
        for slot in &c.in_world_only {
            let _ = writeln!(
                out,
                "    {slot}: validated in world (provider has no JSON Schema)"
            );
        }
        if !c.response.is_empty() {
            let statuses: Vec<String> = c.response.keys().map(|s| s.to_string()).collect();
            if matches!(w.trigger, Trigger::Socket { .. }) {
                // A socket's "response" contract is its outgoing message shape.
                let _ = writeln!(
                    out,
                    "    outgoing messages: validated against the declared contract"
                );
            } else {
                let _ = writeln!(out, "    response: {}", statuses.join(", "));
            }
        }
        if let Some(auth) = &w.auth {
            let _ = writeln!(out, "    auth: {auth} (resolved in world)");
        }
        if !w.errors.is_empty() {
            let errors: Vec<String> = w
                .errors
                .iter()
                .map(|e| format!("{} ({})", e.code, e.status))
                .collect();
            let _ = writeln!(out, "    errors: {}", errors.join(", "));
        }
        if !w.resources.is_empty() {
            let _ = writeln!(out, "    resources:");
            for r in &w.resources {
                let _ = writeln!(out, "      {r}");
            }
        }
        if !w.dispatches.is_empty() {
            let _ = writeln!(out, "    hands work to (invoke or dispatch):");
            for d in &w.dispatches {
                let _ = writeln!(out, "      {d}");
            }
        }
        if !w.publishes.is_empty() {
            let _ = writeln!(out, "    publishes:");
            for topic in &w.publishes {
                let consumed = m
                    .workloads
                    .iter()
                    .any(|c| matches!(&c.trigger, Trigger::Queue { topic: t, .. } if t == topic));
                let _ = writeln!(
                    out,
                    "      {topic}{}",
                    if consumed {
                        ""
                    } else {
                        "  (no consumer in this application)"
                    }
                );
            }
        }
        if let Some(ms) = w.timeout_ms {
            let _ = writeln!(out, "    timeout: {ms}ms");
        }
        if let Some(n) = w.max_concurrency {
            let _ = writeln!(out, "    concurrency: {n}");
        }
    }
    if !m.resources.is_empty() {
        let _ = writeln!(out, "\nResources");
        for r in &m.resources {
            let _ = writeln!(
                out,
                "  {}/{}\n    lifetime: runtime\n    lease: per work",
                r.kind, r.name
            );
            if !r.env.is_empty() {
                let _ = writeln!(out, "    env: {}", r.env.join(", "));
            }
            // The bound an operator sizes against (`docs/runbooks/sizing.md`),
            // with its default made explicit when the declaration left it out.
            if r.kind == "postgres" {
                let max = r.config["pool"]["max"].as_u64();
                let _ = writeln!(
                    out,
                    "    pool.max: {}",
                    max.map_or_else(|| "16 (default)".to_owned(), |n| n.to_string())
                );
            }
        }
    }
    if !m.env.is_empty() {
        let _ = writeln!(out, "\nEnvironment");
        for e in &m.env {
            let _ = writeln!(
                out,
                "  {:<20} {}{}",
                e.name,
                e.kind,
                if e.required { "" } else { " (optional)" }
            );
        }
    }
    out
}

/// What changed between two definitions, for the reload line: added and
/// removed workloads by id (`http:GET /x`, `task:name`). Empty when only
/// handler bodies changed.
pub fn workload_diff(
    previous: Option<&ApplicationDefinition>,
    current: &ApplicationDefinition,
) -> String {
    let Some(previous) = previous else {
        return String::new();
    };
    let before: std::collections::BTreeSet<&str> =
        previous.workloads().iter().map(|w| w.id.as_str()).collect();
    let after: std::collections::BTreeSet<&str> =
        current.workloads().iter().map(|w| w.id.as_str()).collect();
    let mut out = String::new();
    for id in after.difference(&before) {
        let _ = write!(out, "\n  + {id}");
    }
    for id in before.difference(&after) {
        let _ = write!(out, "\n  - {id}");
    }
    out
}
