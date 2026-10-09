//! `agentos scan`: index the repository and refresh the project profile.

use std::fmt::Write as _;
use std::process::ExitCode;

use agentos_config::OutputFormat;
use agentos_knowledge::ScanReport;
use anyhow::Context;

use crate::app::App;
use crate::output;

/// Runs `agentos scan`.
pub fn run(app: &App, format: OutputFormat) -> anyhow::Result<ExitCode> {
    let report = agentos_knowledge::scan(&app.root.path)
        .with_context(|| format!("cannot scan {}", app.root.path.display()))?;
    emit(app, &report, "Scanned", format)
}

/// Runs `agentos knowledge rebuild`.
pub fn rebuild(app: &App, format: OutputFormat) -> anyhow::Result<ExitCode> {
    let report = agentos_knowledge::rebuild(&app.root.path)
        .with_context(|| format!("cannot rebuild {}", app.root.path.display()))?;
    emit(app, &report, "Rebuilt", format)
}

fn emit(
    app: &App,
    report: &ScanReport,
    verb: &str,
    format: OutputFormat,
) -> anyhow::Result<ExitCode> {
    match format {
        OutputFormat::Json => output::json(report)?,
        OutputFormat::Text => {
            output::text(&render(verb, &app.root.path.display().to_string(), report))?
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn render(verb: &str, root: &str, report: &ScanReport) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "{verb} {root}: {} files, {} changed, {} unchanged, {} deleted",
        report.files, report.changed, report.unchanged, report.deleted
    );
    let _ = writeln!(
        out,
        "nodes {}, edges {}, fragments {}",
        report.nodes, report.edges, report.fragments
    );
    let _ = writeln!(out, "rows written {}", report.rows_written);
    let _ = writeln!(out, "profile {}", report.profile);
    out
}
