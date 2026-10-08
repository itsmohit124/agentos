//! `agentos config`: show the effective configuration and the source of each value.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::process::ExitCode;

use agentos_config::{OutputFormat, ResolvedConfig};
use serde::Serialize;

use crate::app::App;
use crate::output;

#[derive(Serialize)]
struct Value {
    value: String,
    source: String,
}

#[derive(Serialize)]
struct Report {
    file: String,
    file_loaded: bool,
    values: BTreeMap<&'static str, Value>,
}

/// Runs `agentos config`. Fails if the configuration is invalid.
pub fn run(app: &App, format: OutputFormat) -> anyhow::Result<ExitCode> {
    let resolved = app.config.as_ref().map_err(|e| anyhow::anyhow!("{e}"))?;
    let report = build(resolved);
    match format {
        OutputFormat::Json => output::json(&report)?,
        OutputFormat::Text => output::text(&render_text(&report))?,
    }
    Ok(ExitCode::SUCCESS)
}

fn build(resolved: &ResolvedConfig) -> Report {
    let values = resolved
        .config
        .entries()
        .into_iter()
        .map(|(key, value)| {
            let source = resolved
                .source(key)
                .map_or_else(|| "unknown".to_owned(), ToString::to_string);
            (key, Value { value, source })
        })
        .collect();
    Report {
        file: resolved.file.display().to_string(),
        file_loaded: resolved.file_loaded,
        values,
    }
}

fn render_text(report: &Report) -> String {
    let mut out = String::new();
    let state = if report.file_loaded {
        "loaded"
    } else {
        "not found"
    };
    let _ = writeln!(out, "# config file: {} ({state})", report.file);
    for (key, v) in &report.values {
        let _ = writeln!(out, "{key} = {:?}  # source: {}", v.value, v.source);
    }
    out
}
