//! `agentos benchmark`: naive token baselines.
//!
//! The baseline counts file tokens. It does not call the context engine.

use std::fmt::Write as _;
use std::path::PathBuf;
use std::process::ExitCode;

use agentos_config::OutputFormat;
use agentos_evaluation::baseline::{self, BaselineReport};
use anyhow::Context;

use crate::app::App;
use crate::cli::BenchmarkCommand;
use crate::output;

/// Runs `agentos benchmark`.
pub fn run(
    app: &App,
    command: &BenchmarkCommand,
    format: OutputFormat,
) -> anyhow::Result<ExitCode> {
    match command {
        BenchmarkCommand::Baseline { fixture } => {
            let fixture_dir = resolve_fixture(&app.root.path, fixture);
            let report = baseline::write(&app.root.path, &fixture_dir)
                .with_context(|| format!("cannot measure {}", fixture_dir.display()))?;
            match format {
                OutputFormat::Json => output::text(&baseline::render(&report)?)?,
                OutputFormat::Text => output::text(&render_text(&report))?,
            }
            Ok(ExitCode::SUCCESS)
        }
    }
}

fn resolve_fixture(root: &std::path::Path, fixture: &std::path::Path) -> PathBuf {
    if fixture.is_absolute() {
        fixture.to_path_buf()
    } else if root.join(fixture).is_dir() {
        root.join(fixture)
    } else {
        fixture.to_path_buf()
    }
}

fn render_text(report: &BaselineReport) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "Baseline: {} ({} files, {} full-repository tokens)",
        report.fixture, report.file_count, report.full_repository_tokens
    );
    let _ = writeln!(out, "heuristic: {}", report.token_heuristic);
    for case in &report.cases {
        let _ = writeln!(
            out,
            "{}  relevant {} tokens  files {}",
            case.id,
            case.relevant_file_tokens,
            case.relevant_files.len()
        );
    }
    let _ = writeln!(
        out,
        "wrote {}/{}",
        baseline::BASELINE_DIR,
        baseline::BASELINE_FILE
    );
    out
}
