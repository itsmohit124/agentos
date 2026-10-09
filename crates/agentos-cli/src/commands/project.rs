//! `agentos project profile`: print the persisted project profile.

use std::fmt::Write as _;
use std::process::ExitCode;

use agentos_config::OutputFormat;
use agentos_knowledge::{self, ProjectProfile};
use anyhow::Context;

use crate::app::App;
use crate::output;

/// Runs `agentos project`.
pub fn run(
    app: &App,
    command: &crate::cli::ProjectCommand,
    format: OutputFormat,
) -> anyhow::Result<ExitCode> {
    match command {
        crate::cli::ProjectCommand::Profile => profile(app, format),
    }
}

fn profile(app: &App, format: OutputFormat) -> anyhow::Result<ExitCode> {
    let path = agentos_knowledge::profile_path(&app.root.path);
    let profile = agentos_knowledge::read_profile(&app.root.path).with_context(|| {
        format!(
            "no project profile at {} (run `agentos init` or `agentos scan`)",
            path.display()
        )
    })?;
    match format {
        OutputFormat::Json => output::json(&profile)?,
        OutputFormat::Text => output::text(&render(&profile))?,
    }
    Ok(ExitCode::SUCCESS)
}

fn render(profile: &ProjectProfile) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "classification: {}", profile.classification.as_str());
    let _ = writeln!(out, "maturity: {}", profile.maturity.as_str());
    let _ = writeln!(out, "confidence_millis: {}", profile.confidence_millis);
    let _ = writeln!(out, "uncertain: {}", profile.uncertain);
    let _ = writeln!(out, "provenance_kind: {}", profile.provenance_kind.as_str());
    let _ = writeln!(out, "signals:");
    for signal in &profile.signals {
        if signal.present {
            let _ = writeln!(out, "  {}: {}", signal.name, signal.evidence.join(", "));
        } else {
            let _ = writeln!(out, "  {}: absent", signal.name);
        }
    }
    out
}
