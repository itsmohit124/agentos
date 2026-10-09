//! `agentos init`: create the `.agentos/` project state idempotently.

use std::fmt::Write as _;
use std::process::ExitCode;

use agentos_config::{DEFAULT_CONFIG_TOML, OutputFormat};
use agentos_core::db::Database;
use anyhow::Context;
use serde::Serialize;

use crate::app::App;
use crate::output;

#[derive(Serialize)]
struct Entry {
    path: String,
    status: &'static str,
}

#[derive(Serialize)]
struct Report {
    root: String,
    root_source: &'static str,
    entries: Vec<Entry>,
    schema_version: u32,
    migrations_applied: Vec<u32>,
}

/// Runs `agentos init`.
pub fn run(app: &App, format: OutputFormat) -> anyhow::Result<ExitCode> {
    let layout = &app.layout;
    let mut entries: Vec<Entry> = layout
        .ensure(&app.fs, DEFAULT_CONFIG_TOML)
        .with_context(|| format!("cannot initialize {}", layout.dir().display()))?
        .into_iter()
        .map(|e| Entry {
            path: e.relative_path.display().to_string(),
            status: e.status.as_str(),
        })
        .collect();

    let db_path = layout.db_file();
    let db_existed = db_path.exists();
    let (_db, migrations) = Database::open(&db_path)
        .with_context(|| format!("cannot open database {}", db_path.display()))?;
    let db_status = match (db_existed, migrations.applied.is_empty()) {
        (false, _) => "created",
        (true, false) => "migrated",
        (true, true) => "exists",
    };
    entries.push(Entry {
        path: layout.relative(&db_path).display().to_string(),
        status: db_status,
    });

    let (_profile, profile_status) = agentos_knowledge::write_profile(&app.root.path)
        .with_context(|| format!("cannot write project profile in {}", layout.dir().display()))?;
    entries.push(Entry {
        path: layout
            .relative(&agentos_knowledge::profile_path(&app.root.path))
            .display()
            .to_string(),
        status: profile_status.as_str(),
    });

    if let Err(err) = &app.config {
        output::warn(&format!("{err} (run `agentos doctor` for details)"));
    }

    let report = Report {
        root: app.root.path.display().to_string(),
        root_source: app.root.source.as_str(),
        entries,
        schema_version: migrations.current_version,
        migrations_applied: migrations.applied,
    };
    match format {
        OutputFormat::Json => output::json(&report)?,
        OutputFormat::Text => output::text(&render_text(&report))?,
    }
    Ok(ExitCode::SUCCESS)
}

fn render_text(report: &Report) -> String {
    let fresh = report.entries.iter().any(|e| e.status != "exists");
    let mut out = String::new();
    let heading = if fresh {
        "Initialized AgentOS project"
    } else {
        "AgentOS project already initialized"
    };
    let _ = writeln!(
        out,
        "{heading} at {} (root: {})",
        report.root, report.root_source
    );
    for e in &report.entries {
        let _ = writeln!(out, "  {:<9}{}", e.status, e.path);
    }
    let applied = if report.migrations_applied.is_empty() {
        "none".to_owned()
    } else {
        report
            .migrations_applied
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(", ")
    };
    let _ = writeln!(
        out,
        "Schema version: {} (migrations applied: {applied})",
        report.schema_version
    );
    out
}
