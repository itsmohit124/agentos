//! `agentos architecture analyze`: detectors over the existing knowledge graph.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use agentos_config::OutputFormat;
use agentos_feedback::{ArchitectureReport, analyze_project, load_project_config};
use anyhow::Context;

use crate::app::App;
use crate::cli::ArchitectureCommand;
use crate::output;

/// Exit code when at least one architecture finding was reported.
const EXIT_FAILED: u8 = 3;

/// Runs `agentos architecture`.
pub fn run(
    app: &App,
    command: &ArchitectureCommand,
    format: OutputFormat,
) -> anyhow::Result<ExitCode> {
    match command {
        ArchitectureCommand::Analyze { path } => {
            let root = resolve_target(app, path.as_deref())?;
            let config =
                load_project_config(&root.join(".agentos/config.toml")).with_context(|| {
                    format!("cannot read architecture config in {}", root.display())
                })?;
            let report = analyze_project(&root, &config.architecture)
                .with_context(|| format!("cannot analyze {}", root.display()))?;
            let failed = !report.findings.is_empty();
            let text = render(&report);
            match format {
                OutputFormat::Json => output::json(&report)?,
                OutputFormat::Text => output::text(&text)?,
            }
            Ok(if failed {
                ExitCode::from(EXIT_FAILED)
            } else {
                ExitCode::SUCCESS
            })
        }
    }
}

fn resolve_target(app: &App, path: Option<&Path>) -> anyhow::Result<PathBuf> {
    match path {
        None => Ok(app.root.path.clone()),
        Some(path) => {
            let full = if path.is_absolute() {
                path.to_path_buf()
            } else {
                std::env::current_dir()
                    .context("cannot determine the current directory")?
                    .join(path)
            };
            full.canonicalize()
                .with_context(|| format!("architecture target {} does not exist", full.display()))
        }
    }
}

fn render(report: &ArchitectureReport) -> String {
    let mut out = String::new();
    if report.findings.is_empty() {
        return "no architecture findings\n".to_owned();
    }
    for finding in &report.findings {
        let paths = if finding.paths.is_empty() {
            "-".to_owned()
        } else {
            finding.paths.join(",")
        };
        let _ = writeln!(out, "{}  {paths}", finding.detector);
        let _ = writeln!(out, "  {}", finding.summary);
    }
    let _ = writeln!(out, "{} findings", report.findings.len());
    out
}
