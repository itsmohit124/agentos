//! `agentos evaluate <area>`: run a deterministic evaluation suite.

use std::fmt::Write as _;
use std::process::ExitCode;

use agentos_config::OutputFormat;
use agentos_evaluation::EvalReport;
use serde::Serialize;

use crate::app::App;
use crate::cli::{EvalArea, EvaluateArgs};
use crate::output;

/// Exit code when at least one case fails.
const EXIT_FAILED: u8 = 3;

#[derive(Serialize)]
struct Out<'a> {
    ok: bool,
    passed: usize,
    failed: usize,
    by_kind: std::collections::BTreeMap<String, agentos_evaluation::KindMetrics>,
    #[serde(flatten)]
    report: &'a EvalReport,
}

/// Runs `agentos evaluate`.
pub fn run(app: &App, args: &EvaluateArgs, format: OutputFormat) -> anyhow::Result<ExitCode> {
    let report = match args.area {
        EvalArea::Skills => {
            let cases = args
                .evals_dir
                .clone()
                .unwrap_or_else(|| app.root.path.join("evals/skills"));
            let fixtures = args
                .fixtures_dir
                .clone()
                .unwrap_or_else(|| app.root.path.join("fixtures"));
            agentos_evaluation::skills::run(&cases, &fixtures, agentos_skills::builtin::location())?
        }
        EvalArea::Knowledge => {
            let cases = args
                .evals_dir
                .clone()
                .unwrap_or_else(|| app.root.path.join("evals/knowledge"));
            let fixtures = args
                .fixtures_dir
                .clone()
                .unwrap_or_else(|| app.root.path.join("fixtures"));
            agentos_evaluation::knowledge::run(&cases, &fixtures)?
        }
        EvalArea::Ddd => {
            let cases = args
                .evals_dir
                .clone()
                .unwrap_or_else(|| app.root.path.join("evals/ddd"));
            let fixtures = args
                .fixtures_dir
                .clone()
                .unwrap_or_else(|| app.root.path.join("fixtures"));
            agentos_evaluation::ddd::run(&cases, &fixtures)?
        }
        EvalArea::Context => {
            let cases = args
                .evals_dir
                .clone()
                .unwrap_or_else(|| app.root.path.join("evals/context"));
            let fixtures = args
                .fixtures_dir
                .clone()
                .unwrap_or_else(|| app.root.path.join("fixtures"));
            agentos_evaluation::context::run(&cases, &fixtures)?
        }
        EvalArea::Feedback => {
            let cases = args
                .evals_dir
                .clone()
                .unwrap_or_else(|| app.root.path.join("evals/feedback"));
            let fixtures = args
                .fixtures_dir
                .clone()
                .unwrap_or_else(|| app.root.path.join("fixtures"));
            agentos_evaluation::feedback::run(&cases, &fixtures)?
        }
        EvalArea::Architecture => {
            let cases = args
                .evals_dir
                .clone()
                .unwrap_or_else(|| app.root.path.join("evals/architecture"));
            let fixtures = args
                .fixtures_dir
                .clone()
                .unwrap_or_else(|| app.root.path.join("fixtures"));
            agentos_evaluation::architecture::run(&cases, &fixtures)?
        }
        EvalArea::Workflow => {
            let cases = args
                .evals_dir
                .clone()
                .unwrap_or_else(|| app.root.path.join("evals/workflow"));
            let fixtures = args
                .fixtures_dir
                .clone()
                .unwrap_or_else(|| app.root.path.join("fixtures"));
            agentos_evaluation::workflow::run(&cases, &fixtures)?
        }
    };
    match format {
        OutputFormat::Json => output::json(&Out {
            ok: report.is_ok(),
            passed: report.passed(),
            failed: report.failed(),
            by_kind: report.by_kind(),
            report: &report,
        })?,
        OutputFormat::Text => output::text(&render_text(&report))?,
    }
    Ok(if report.is_ok() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(EXIT_FAILED)
    })
}

fn render_text(report: &EvalReport) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "Evaluation: {} ({})",
        report.area,
        report.cases_dir.display()
    );
    for c in &report.cases {
        let _ = writeln!(
            out,
            "  [{}] {:<10} {:<40} {}",
            if c.passed { "pass" } else { "FAIL" },
            c.kind,
            c.id,
            c.detail
        );
    }
    let _ = writeln!(out, "By kind:");
    for (kind, m) in report.by_kind() {
        let _ = writeln!(out, "  {kind:<10} {}/{}", m.passed, m.total);
    }
    let _ = writeln!(
        out,
        "Result: {} ({}/{} passed)",
        if report.is_ok() { "OK" } else { "FAILED" },
        report.passed(),
        report.cases.len()
    );
    out
}
