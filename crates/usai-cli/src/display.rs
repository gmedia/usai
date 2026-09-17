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
        (_, Trigger::Service) => "service",
    }
}

/// The compact banner `usai dev` prints.
pub fn banner(
    definition: &ApplicationDefinition,
    revision: &str,
    base_url: Option<&str>,
    status: Option<&RuntimeStatus>,
    docs: bool,
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
            if let Trigger::Http { method, path, raw } = &w.trigger {
                let _ = writeln!(
                    out,
                    "  {method:<6} {path}{}",
                    if *raw { "   (raw)" } else { "" }
                );
            }
        }
    }
    for (title, kind) in [
        ("Tasks", "task"),
        ("Commands", "command"),
        ("Services", "service"),
    ] {
        let items: Vec<_> = m
            .workloads
            .iter()
            .filter(|w| w.trigger.kind_name() == kind)
            .collect();
        if !items.is_empty() {
            let _ = writeln!(out, "\n{title}");
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
        let _ = writeln!(out, "\nCron");
        for w in cron {
            if let Trigger::Cron { schedule, .. } = &w.trigger {
                let _ = writeln!(out, "  {:<16} {schedule}", w.name);
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
        if docs {
            let _ = writeln!(out, "API Docs  {url}/_usai/docs");
            let _ = writeln!(out, "OpenAPI   {url}/_usai/openapi.json");
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
    let mut last_kind = "";
    for w in &m.workloads {
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
            Trigger::Http { method, path, raw } => {
                let _ = writeln!(
                    out,
                    "  {method} {path}{}",
                    if *raw {
                        "  (raw: contracts and docs unavailable)"
                    } else {
                        ""
                    }
                );
            }
            Trigger::Cron {
                schedule, overlap, ..
            } => {
                let _ = writeln!(
                    out,
                    "  {}\n    schedule: {schedule}\n    overlap: {overlap:?}",
                    w.name
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
                let _ = writeln!(out, "    {slot}: validated before world creation");
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
            let _ = writeln!(out, "    response: {}", statuses.join(", "));
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
            let _ = writeln!(out, "    dispatches:");
            for d in &w.dispatches {
                let _ = writeln!(out, "      {d}");
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
